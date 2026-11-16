package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	pb "example.com/gamenode/plugin-example/proto"
	"google.golang.org/grpc"
	"google.golang.org/protobuf/proto"
)

const (
	pluginID      = "org.example.gamenode.configuration-go"
	providerID    = "configuration-go"
	preferenceKey = "preferred-catalog-label"
)

type windowResult struct {
	value string
	saved bool
	err   error
}

type processWindow struct {
	id       int
	windowID string
	cmd      *exec.Cmd
	done     chan struct{}
	resultMu sync.Mutex
	result   windowResult
}

type preferences interface {
	Get(context.Context, string) (string, uint64, error)
	Put(context.Context, string, uint64, string) (uint64, bool, error)
}

type windowDriver interface {
	Open(string) (*processWindow, error)
	Focus(int) error
	Close(*processWindow) error
}

type configurationWindows struct {
	mu      sync.Mutex
	current *processWindow
	store   preferences
	driver  windowDriver
}

func (w *configurationWindows) open(ctx context.Context, id string) (bool, string) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if current := w.current; current != nil {
		select {
		case <-current.done:
			w.current = nil
		default:
			if current.windowID != id {
				return false, "window-already-open"
			}
			if err := w.driver.Focus(current.id); err != nil {
				return false, "focus-failed"
			}
			return true, ""
		}
	}
	readCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	value, _, err := w.store.Get(readCtx, preferenceKey)
	if err != nil {
		return false, "preference-read-failed"
	}
	window, err := w.driver.Open(value)
	if err != nil {
		return false, "window-open-failed"
	}
	window.windowID = id
	w.current = window
	go w.persistWhenClosed(window)
	return true, ""
}

func (w *configurationWindows) focus(id string) (bool, string) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.current == nil || w.current.windowID != id {
		return false, "window-closed"
	}
	select {
	case <-w.current.done:
		w.current = nil
		return false, "window-closed"
	default:
	}
	if err := w.driver.Focus(w.current.id); err != nil {
		return false, "focus-failed"
	}
	return true, ""
}

func (w *configurationWindows) close(id string) (bool, string) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.current == nil {
		return true, ""
	}
	if w.current.windowID != id {
		return false, "unknown-window-id"
	}
	if err := w.driver.Close(w.current); err != nil {
		return false, "window-close-failed"
	}
	w.current = nil
	return true, ""
}

func (w *configurationWindows) persistWhenClosed(window *processWindow) {
	<-window.done
	window.resultMu.Lock()
	result := window.result
	window.resultMu.Unlock()
	w.mu.Lock()
	if w.current == window {
		w.current = nil
	}
	w.mu.Unlock()
	if result.err != nil || !result.saved {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	_, revision, err := w.store.Get(ctx, preferenceKey)
	if err != nil {
		log.Printf("preference read failed: %v", err)
		return
	}
	for attempt := 0; attempt < 2; attempt++ {
		_, conflict, err := w.store.Put(ctx, preferenceKey, revision, result.value)
		if err != nil {
			log.Printf("preference save failed: %v", err)
			return
		}
		if !conflict {
			return
		}
		_, revision, err = w.store.Get(ctx, preferenceKey)
		if err != nil {
			log.Printf("preference reread failed: %v", err)
			return
		}
	}
}

type nativeWindowDriver struct{}

func (nativeWindowDriver) Open(preference string) (*processWindow, error) {
	var cmd *exec.Cmd
	if runtime.GOOS == "windows" {
		script := filepath.Join(exampleRoot(), "plugins", "shared", "config-window.ps1")
		cmd = exec.Command("powershell.exe", "-NoProfile", "-STA", "-File", script, preference)
	} else if runtime.GOOS == "linux" {
		script := filepath.Join(exampleRoot(), "plugins", "shared", "config-window.sh")
		cmd = exec.Command("sh", script, preference)
	} else {
		return nil, fmt.Errorf("unsupported OS %s", runtime.GOOS)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	cmd.Stderr = io.Discard
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	window := &processWindow{id: cmd.Process.Pid, cmd: cmd, done: make(chan struct{})}
	go func() {
		bytes, readErr := io.ReadAll(io.LimitReader(stdout, 4097))
		waitErr := cmd.Wait()
		result := windowResult{value: strings.TrimSuffix(string(bytes), "\n"), saved: waitErr == nil}
		if readErr != nil {
			result.err = readErr
		}
		if waitErr != nil {
			result.err = waitErr
		}
		window.resultMu.Lock()
		window.result = result
		window.resultMu.Unlock()
		close(window.done)
	}()
	return window, nil
}

func (nativeWindowDriver) Focus(pid int) error {
	var cmd *exec.Cmd
	if runtime.GOOS == "windows" {
		script := filepath.Join(exampleRoot(), "plugins", "shared", "focus-window.ps1")
		cmd = exec.Command("powershell.exe", "-NoProfile", "-File", script, fmt.Sprint(pid))
	} else {
		list, err := exec.Command("wmctrl", "-lp").Output()
		if err != nil {
			return err
		}
		var windowID string
		for _, line := range strings.Split(string(list), "\n") {
			fields := strings.Fields(line)
			if len(fields) >= 3 && fields[2] == fmt.Sprint(pid) {
				windowID = fields[0]
				break
			}
		}
		if windowID == "" {
			return errors.New("native window not found")
		}
		cmd = exec.Command("wmctrl", "-ia", windowID)
	}
	return cmd.Run()
}

func (nativeWindowDriver) Close(window *processWindow) error { return window.cmd.Process.Kill() }

func exampleRoot() string {
	if executable, err := os.Executable(); err == nil {
		for current := filepath.Dir(executable); ; current = filepath.Dir(current) {
			if _, err := os.Stat(filepath.Join(current, "plugins", "shared", "config-window.sh")); err == nil {
				return current
			}
			if filepath.Dir(current) == current {
				break
			}
		}
	}
	dir, _ := os.Getwd()
	for current := dir; ; current = filepath.Dir(current) {
		if _, err := os.Stat(filepath.Join(current, "examples", "plugins", "shared", "config-window.sh")); err == nil {
			return filepath.Join(current, "examples")
		}
		if filepath.Dir(current) == current {
			break
		}
	}
	return filepath.Clean(filepath.Join("examples", "go", "..", ".."))
}

type host struct {
	conn       *grpc.ClientConn
	resource   *pb.ServiceContext
	credential []byte
}

func (h *host) invoke(ctx context.Context, method string, request, reply proto.Message) error {
	return h.conn.Invoke(ctx, "/gamenode.plugin.v1.ResourceHost/"+method, request, reply)
}

func (h *host) Get(ctx context.Context, key string) (string, uint64, error) {
	request := &pb.StorageGetRequest{Context: h.resource, Key: key}
	reply := new(pb.StorageGetReply)
	if err := h.invoke(ctx, "StorageGet", request, reply); err != nil {
		return "", 0, err
	}
	if !reply.Found {
		return "Games", 0, nil
	}
	return string(reply.Value), reply.Revision, nil
}

func (h *host) Put(ctx context.Context, key string, revision uint64, value string) (uint64, bool, error) {
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return revision, false, err
	}
	request := &pb.StoragePutRequest{
		Mutation: &pb.MutationContext{
			Context:          h.resource,
			IdempotencyKey:   hex.EncodeToString(random[:]),
			ExpectedRevision: revision,
		},
		Key:        key,
		Value:      []byte(value),
		Durability: pb.StorageDurability_STORAGE_DURABILITY_PERSISTENT,
	}
	reply := new(pb.StorageMutationReply)
	if err := h.invoke(ctx, "StoragePut", request, reply); err != nil {
		return revision, false, err
	}
	return reply.Revision, reply.Outcome == pb.MutationOutcome_MUTATION_OUTCOME_REVISION_CONFLICT, nil
}

func resultFor(command *pb.ProviderCommand, manager *configurationWindows) *pb.OperationResult {
	result := &pb.OperationResult{Disposition: pb.OperationDisposition_OPERATION_DISPOSITION_COMPLETED}
	switch command.Operation {
	case pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_OPEN,
		pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_FOCUS,
		pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_CLOSE:
		accepted, failure := false, "unsupported-operation"
		switch command.Operation {
		case pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_OPEN:
			accepted, failure = manager.open(context.Background(), command.TargetOperationId)
		case pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_FOCUS:
			accepted, failure = manager.focus(command.TargetOperationId)
		case pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_CLOSE:
			accepted, failure = manager.close(command.TargetOperationId)
		}
		result.Configuration = &pb.ConfigurationResult{
			SessionId: command.Context.SessionId,
			WindowId:  command.TargetOperationId,
			Accepted:  accepted,
			Failure:   failure,
		}
		if !accepted {
			result.Disposition = pb.OperationDisposition_OPERATION_DISPOSITION_FAILED
		}
	case pb.ProviderOperation_PROVIDER_OPERATION_DISCOVER:
		result.Discovery = &pb.DiscoveryResult{}
	case pb.ProviderOperation_PROVIDER_OPERATION_SYNC_CATALOG:
		result.Catalogs = []*pb.CatalogCompletion{{
			Outcome: &pb.CatalogCompletion_CompletedRecords{CompletedRecords: 0},
		}}
	case pb.ProviderOperation_PROVIDER_OPERATION_READ_COUNTER:
		result.Counters = []*pb.CounterObservation{}
	case pb.ProviderOperation_PROVIDER_OPERATION_SHUTDOWN:
		result.Disposition = pb.OperationDisposition_OPERATION_DISPOSITION_COMPLETED
	}
	return result
}

func main() {
	endpoint := os.Getenv("GAMENODE_PLUGIN_ENDPOINT")
	credential, err := hex.DecodeString(os.Getenv("GAMENODE_PLUGIN_SESSION_CREDENTIAL"))
	if err != nil || len(credential) != 32 {
		log.Fatal("invalid plugin session context")
	}
	conn, err := dialPlugin(endpoint)
	if err != nil {
		log.Fatal(err)
	}
	defer conn.Close()
	openCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	open := &pb.OpenSessionRequest{
		PluginId: pluginID, PluginVersion: "0.1.0", ApiMajor: 1, ApiMinor: 6,
		MinApiMajor: 1, MinApiMinor: 6, MaxApiMajor: 1, MaxApiMinor: 6,
		SessionCredential:     credential,
		RequestedCapabilities: []string{"ui.configure", "storage.read", "storage.write", "storage.persist"},
		SessionKind:           pb.SessionKind_SESSION_KIND_PROVIDER,
	}
	opened := new(pb.OpenSessionReply)
	if err := conn.Invoke(openCtx, "/gamenode.plugin.v1.PluginHost/OpenSession", open, opened); err != nil {
		log.Fatal(err)
	}
	if opened.ApiMinor < 6 || opened.ResourceContext == nil {
		log.Fatal("host did not negotiate API 1.6 resource context")
	}
	base := &pb.ServiceContext{
		SessionId: opened.SessionId, SessionCredential: credential, ProviderId: providerID,
		OperationId: os.Getenv("GAMENODE_PLUGIN_OPERATION_ID"), SessionKind: pb.SessionKind_SESSION_KIND_PROVIDER,
	}
	stream, err := conn.NewStream(context.Background(), &grpc.StreamDesc{ClientStreams: true, ServerStreams: true}, "/gamenode.plugin.v1.ProviderHost/Exchange")
	if err != nil {
		log.Fatal(err)
	}
	if err := stream.SendMsg(&pb.ProviderEvent{Context: base, Event: &pb.ProviderEvent_Ready{Ready: true}}); err != nil {
		log.Fatal(err)
	}
	store := &host{conn: conn, resource: opened.ResourceContext, credential: credential}
	windows := &configurationWindows{store: store, driver: nativeWindowDriver{}}
	for {
		command := new(pb.ProviderCommand)
		if err := stream.RecvMsg(command); err != nil {
			if errors.Is(err, io.EOF) {
				return
			}
			log.Fatal(err)
		}
		result := resultFor(command, windows)
		if err := stream.SendMsg(&pb.ProviderEvent{Context: command.Context, Event: &pb.ProviderEvent_Result{Result: result}}); err != nil {
			log.Fatal(err)
		}
		if command.Operation == pb.ProviderOperation_PROVIDER_OPERATION_SHUTDOWN {
			return
		}
	}
}
