package provider

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	"example.com/gamenode/plugins/go-grpc/proto"
	pbproto "google.golang.org/protobuf/proto"
)

const preferenceKey = "preferred-catalog-label"
const maxPreferenceBytes = 4096

// PreferenceStore keeps the example's non-secret preference in host-owned storage.
type PreferenceStore interface {
	Get(context.Context, *proto.ServiceContext, string) (*proto.StorageGetReply, error)
	Put(context.Context, *proto.ServiceContext, string, []byte, uint64) (*proto.StorageMutationReply, error)
}

type resourcePreferenceStore struct{ client proto.ResourceHostClient }

func (s resourcePreferenceStore) Get(ctx context.Context, service *proto.ServiceContext, key string) (*proto.StorageGetReply, error) {
	return s.client.StorageGet(ctx, &proto.StorageGetRequest{
		Context: pbproto.Clone(service).(*proto.ServiceContext),
		Key:     key,
	})
}

func (s resourcePreferenceStore) Put(ctx context.Context, service *proto.ServiceContext, key string, value []byte, revision uint64) (*proto.StorageMutationReply, error) {
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return nil, fmt.Errorf("create storage idempotency key: %w", err)
	}
	return s.client.StoragePut(ctx, &proto.StoragePutRequest{
		Mutation: &proto.MutationContext{
			Context:          pbproto.Clone(service).(*proto.ServiceContext),
			OperationId:      service.GetOperationId(),
			IdempotencyKey:   hex.EncodeToString(nonce[:]),
			ExpectedRevision: revision,
		},
		Key:        key,
		Value:      append([]byte(nil), value...),
		Durability: proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT,
	})
}

type WindowExit struct {
	Preference []byte
	Saved      bool
	Err        error
}

// ConfigWindow is one plugin-owned native configuration process.
type ConfigWindow interface {
	Focus(context.Context) bool
	Stop(context.Context) error
	Done() <-chan struct{}
	Result() WindowExit
}

type WindowLauncher interface {
	Start(string) (ConfigWindow, error)
}

type managedConfigWindow struct {
	id       string
	window   ConfigWindow
	finished chan struct{}
	err      error
}

// ConfigurationLifecycle provides one correlated configuration window per provider session.
type ConfigurationLifecycle struct {
	mu       sync.Mutex
	store    PreferenceStore
	launcher WindowLauncher
	service  *proto.ServiceContext
	current  *managedConfigWindow
}

func NewConfigurationLifecycle(store PreferenceStore, launcher WindowLauncher, service *proto.ServiceContext) *ConfigurationLifecycle {
	return &ConfigurationLifecycle{
		store: store, launcher: launcher,
		service: pbproto.Clone(service).(*proto.ServiceContext),
	}
}

func (l *ConfigurationLifecycle) Handle(ctx context.Context, command *proto.ProviderCommand) *proto.ProviderEvent {
	var result *proto.OperationResult
	switch command.GetOperation() {
	case proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_OPEN:
		result = l.openOrFocus(ctx, command)
	case proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_FOCUS:
		result = l.focus(ctx, command)
	case proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_CLOSE:
		result = l.close(ctx, command)
	default:
		result = configurationResult(command, false, "unsupported-configuration-operation")
	}
	return &proto.ProviderEvent{
		Context: pbproto.Clone(command.GetContext()).(*proto.ServiceContext),
		Event:   &proto.ProviderEvent_Result{Result: result},
	}
}

func (l *ConfigurationLifecycle) openOrFocus(ctx context.Context, command *proto.ProviderCommand) *proto.OperationResult {
	windowID := command.GetTargetOperationId()
	if windowID == "" {
		return configurationResult(command, false, "missing-window-id")
	}

	l.mu.Lock()
	if current := l.current; current != nil {
		select {
		case <-current.finished:
			l.current = nil
		default:
			select {
			case <-current.window.Done():
				l.mu.Unlock()
				select {
				case <-current.finished:
					return l.openOrFocus(ctx, command)
				case <-ctx.Done():
					return configurationResult(command, false, "preference-save-timeout")
				}
			default:
			}
			if current.id != windowID {
				l.mu.Unlock()
				return configurationResult(command, false, "window-already-open")
			}
			l.mu.Unlock()
			accepted := current.window.Focus(ctx)
			return configurationResult(command, accepted, map[bool]string{true: "", false: "focus-failed"}[accepted])
		}
	}

	preference, err := l.readPreference(ctx)
	if err != nil {
		l.mu.Unlock()
		return configurationResult(command, false, "preference-read-failed")
	}
	window, err := l.launcher.Start(preference)
	if err != nil {
		l.mu.Unlock()
		return configurationResult(command, false, "could-not-open-window")
	}
	managed := &managedConfigWindow{id: windowID, window: window, finished: make(chan struct{})}
	l.current = managed
	l.mu.Unlock()
	go l.watchWindow(managed)
	return configurationResult(command, true, "")
}

func (l *ConfigurationLifecycle) focus(ctx context.Context, command *proto.ProviderCommand) *proto.OperationResult {
	l.mu.Lock()
	current := l.current
	if current == nil || current.id != command.GetTargetOperationId() {
		l.mu.Unlock()
		return configurationResult(command, false, "window-closed")
	}
	select {
	case <-current.finished:
		l.mu.Unlock()
		return configurationResult(command, false, "window-closed")
	default:
	}
	l.mu.Unlock()
	accepted := current.window.Focus(ctx)
	failure := "focus-failed"
	if accepted {
		failure = ""
	}
	return configurationResult(command, accepted, failure)
}

func (l *ConfigurationLifecycle) close(ctx context.Context, command *proto.ProviderCommand) *proto.OperationResult {
	l.mu.Lock()
	current := l.current
	if current == nil {
		l.mu.Unlock()
		return configurationResult(command, true, "")
	}
	if current.id != command.GetTargetOperationId() {
		l.mu.Unlock()
		return configurationResult(command, false, "unknown-window-id")
	}
	l.mu.Unlock()

	if err := current.window.Stop(ctx); err != nil {
		return configurationResult(command, false, "window-close-failed")
	}
	select {
	case <-current.finished:
		if current.err != nil {
			return configurationResult(command, false, "preference-save-failed")
		}
		l.mu.Lock()
		if l.current == current {
			l.current = nil
		}
		l.mu.Unlock()
		return configurationResult(command, true, "")
	case <-ctx.Done():
		return configurationResult(command, false, "window-close-timeout")
	}
}

func (l *ConfigurationLifecycle) readPreference(ctx context.Context) (string, error) {
	requestCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	stored, err := l.store.Get(requestCtx, l.service, preferenceKey)
	if err != nil {
		return "", err
	}
	if !stored.GetFound() {
		return "Games", nil
	}
	return string(stored.GetValue()), nil
}

func (l *ConfigurationLifecycle) savePreference(value []byte) error {
	for attempt := 0; attempt < 2; attempt++ {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		stored, err := l.store.Get(ctx, l.service, preferenceKey)
		cancel()
		if err != nil {
			return err
		}
		ctx, cancel = context.WithTimeout(context.Background(), 2*time.Second)
		result, err := l.store.Put(ctx, l.service, preferenceKey, value, stored.GetRevision())
		cancel()
		if err != nil {
			return err
		}
		if result.GetOutcome() != proto.MutationOutcome_MUTATION_OUTCOME_REVISION_CONFLICT {
			return nil
		}
	}
	return errors.New("preference changed while saving")
}

func (l *ConfigurationLifecycle) watchWindow(current *managedConfigWindow) {
	<-current.window.Done()
	exit := current.window.Result()
	if exit.Saved && exit.Err == nil {
		if len(exit.Preference) > maxPreferenceBytes {
			current.err = errors.New("preference exceeds the example size limit")
		} else {
			current.err = l.savePreference(exit.Preference)
		}
	}
	l.mu.Lock()
	if l.current == current {
		// Keep the completed window until its correlated Close or the next Open observes it.
	}
	close(current.finished)
	l.mu.Unlock()
}

func (l *ConfigurationLifecycle) Shutdown(ctx context.Context) {
	l.mu.Lock()
	current := l.current
	l.mu.Unlock()
	if current == nil {
		return
	}
	_ = current.window.Stop(ctx)
	select {
	case <-current.finished:
	case <-ctx.Done():
	}
}

func configurationResult(command *proto.ProviderCommand, accepted bool, failure string) *proto.OperationResult {
	context := pbproto.Clone(command.GetContext()).(*proto.ServiceContext)
	disposition := proto.OperationDisposition_OPERATION_DISPOSITION_FAILED
	if accepted {
		disposition = proto.OperationDisposition_OPERATION_DISPOSITION_COMPLETED
	}
	return &proto.OperationResult{
		Disposition: disposition,
		Configuration: &proto.ConfigurationResult{
			SessionId: context.GetSessionId(),
			ProfileId: "",
			WindowId:  command.GetTargetOperationId(),
			Accepted:  accepted,
			Failure:   failure,
		},
	}
}

type systemWindowLauncher struct{}

func (systemWindowLauncher) Start(preference string) (ConfigWindow, error) {
	var command *exec.Cmd
	if runtime.GOOS == "windows" {
		command = exec.Command("powershell.exe", "-NoProfile", "-STA", "-File", helperPath("config-window.ps1"), preference)
	} else if runtime.GOOS == "linux" {
		command = exec.Command("sh", helperPath("config-window.sh"), preference)
	} else {
		return nil, errors.New("unsupported OS")
	}
	command.Stdin = nil
	var stdout bytes.Buffer
	command.Stdout = &stdout
	command.Stderr = io.Discard
	if err := command.Start(); err != nil {
		return nil, err
	}
	window := &systemConfigWindow{command: command, done: make(chan struct{})}
	go func() {
		err := command.Wait()
		window.exit = WindowExit{Preference: append([]byte(nil), stdout.Bytes()...), Saved: err == nil, Err: err}
		close(window.done)
	}()
	return window, nil
}

type systemConfigWindow struct {
	command *exec.Cmd
	done    chan struct{}
	exit    WindowExit
}

func (w *systemConfigWindow) Done() <-chan struct{} { return w.done }

func (w *systemConfigWindow) Result() WindowExit { return w.exit }

func (w *systemConfigWindow) Focus(ctx context.Context) bool {
	if w.command.Process == nil {
		return false
	}
	var command *exec.Cmd
	if runtime.GOOS == "windows" {
		command = exec.CommandContext(ctx, "powershell.exe", "-NoProfile", "-File", helperPath("focus-window.ps1"), fmt.Sprint(w.command.Process.Pid))
	} else if runtime.GOOS == "linux" {
		output, err := exec.CommandContext(ctx, "wmctrl", "-lp").Output()
		if err != nil {
			return false
		}
		var id string
		for _, line := range strings.Split(string(output), "\n") {
			fields := strings.Fields(line)
			if len(fields) > 2 && fields[2] == fmt.Sprint(w.command.Process.Pid) {
				id = fields[0]
				break
			}
		}
		if id == "" {
			return false
		}
		command = exec.CommandContext(ctx, "wmctrl", "-ia", id)
	} else {
		return false
	}
	return command.Run() == nil
}

func (w *systemConfigWindow) Stop(ctx context.Context) error {
	select {
	case <-w.done:
		return nil
	default:
	}
	if w.command.Process == nil {
		return nil
	}
	if err := w.command.Process.Kill(); err != nil && !errors.Is(err, os.ErrProcessDone) {
		return err
	}
	select {
	case <-w.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func helperPath(name string) string {
	if executable, err := os.Executable(); err == nil {
		// A packaged plugin carries the window helpers in its signed payload:
		// <package>/bin/<os>/<binary> -> <package>/shared/<name>.
		packageRoot := filepath.Dir(filepath.Dir(filepath.Dir(executable)))
		if candidate := filepath.Join(packageRoot, "shared", name); isRegular(candidate) {
			return candidate
		}
		// A host-wide copy under <data>/plugins/shared remains usable for development.
		for parent := filepath.Dir(executable); parent != filepath.Dir(parent); parent = filepath.Dir(parent) {
			candidate := filepath.Join(parent, "plugins", "shared", name)
			if isRegular(candidate) {
				return candidate
			}
		}
	}
	_, file, _, ok := runtime.Caller(0)
	if ok {
		return filepath.Join(filepath.Dir(file), "..", "shared", name)
	}
	return name
}

func isRegular(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}
