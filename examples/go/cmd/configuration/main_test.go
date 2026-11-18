package main

import (
	"context"
	"sync"
	"testing"
	"time"

	pb "example.com/gamenode/plugin-example/proto"
)

type fakePreferences struct {
	mu       sync.Mutex
	value    string
	revision uint64
}

func (f *fakePreferences) Get(context.Context, string) (string, uint64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.value, f.revision, nil
}

func (f *fakePreferences) Put(_ context.Context, _ string, revision uint64, value string) (uint64, bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if revision != f.revision {
		return f.revision, true, nil
	}
	f.value, f.revision = value, f.revision+1
	return f.revision, false, nil
}

type fakeWindows struct {
	mu      sync.Mutex
	next    int
	focused []int
	closed  []int
	opened  []string
}

func (f *fakeWindows) Open(value string) (*processWindow, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.next++
	window := &processWindow{id: f.next, done: make(chan struct{})}
	f.opened = append(f.opened, value)
	return window, nil
}
func (f *fakeWindows) Focus(id int) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.focused = append(f.focused, id)
	return nil
}
func (f *fakeWindows) Close(window *processWindow) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.closed = append(f.closed, window.id)
	return nil
}

func TestConfigurationWindowFocusCloseAndPersistentPreference(t *testing.T) {
	store := &fakePreferences{value: "Games", revision: 1}
	driver := &fakeWindows{}
	manager := &configurationWindows{store: store, driver: driver}

	if accepted, failure := manager.open(context.Background(), "window-1"); !accepted || failure != "" {
		t.Fatalf("open returned %v, %q", accepted, failure)
	}
	if accepted, failure := manager.focus("window-1"); !accepted || failure != "" {
		t.Fatalf("focus returned %v, %q", accepted, failure)
	}
	if accepted, failure := manager.open(context.Background(), "window-1"); !accepted || failure != "" {
		t.Fatalf("repeated Configure did not focus the existing window: %v, %q", accepted, failure)
	}
	if len(driver.opened) != 1 || len(driver.focused) != 2 || driver.opened[0] != "Games" {
		t.Fatalf("window lifecycle mismatch: opened=%v focused=%v", driver.opened, driver.focused)
	}

	window := manager.current
	window.resultMu.Lock()
	window.result = windowResult{value: "Library", saved: true}
	window.resultMu.Unlock()
	close(window.done)
	select {
	case <-waitForPreference(manager, store):
	case <-time.After(time.Second):
		t.Fatal("preference was not saved after the native window closed")
	}
	value, _, _ := store.Get(context.Background(), preferenceKey)
	if value != "Library" {
		t.Fatalf("persistent preference = %q, want Library", value)
	}
	if accepted, failure := manager.close("window-1"); !accepted || failure != "" {
		t.Fatalf("closing an already closed window returned %v, %q", accepted, failure)
	}
}

func TestConfigurationWindowRejectsForeignHandle(t *testing.T) {
	driver := &fakeWindows{}
	manager := &configurationWindows{store: &fakePreferences{value: "Games"}, driver: driver}
	if ok, _ := manager.open(context.Background(), "window-a"); !ok {
		t.Fatal("open failed")
	}
	if ok, reason := manager.close("window-b"); ok || reason != "unknown-window-id" {
		t.Fatalf("foreign close returned %v, %q", ok, reason)
	}
	if ok, reason := manager.open(context.Background(), "window-b"); ok || reason != "window-already-open" {
		t.Fatalf("second window was opened: %v, %q", ok, reason)
	}
}

func TestProviderCommandsStayServiceableWhileConfigurationIsOpen(t *testing.T) {
	driver := &fakeWindows{}
	manager := &configurationWindows{store: &fakePreferences{value: "Games"}, driver: driver}
	ctx := &pb.ServiceContext{SessionId: "fixture-session", OperationId: "open", ProfileEpoch: 4}
	open := resultFor(&pb.ProviderCommand{
		Context: ctx, Operation: pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_OPEN,
		TargetOperationId: "window-handle",
	}, manager)
	if !open.Configuration.Accepted || open.Configuration.SessionId != "fixture-session" || open.Configuration.WindowId != "window-handle" {
		t.Fatalf("configuration response lost correlation: %#v", open.Configuration)
	}
	refresh := resultFor(&pb.ProviderCommand{
		Context: ctx, Operation: pb.ProviderOperation_PROVIDER_OPERATION_SYNC_CATALOG,
	}, manager)
	if refresh.Disposition != pb.OperationDisposition_OPERATION_DISPOSITION_COMPLETED || len(refresh.Catalogs) != 1 {
		t.Fatalf("provider refresh did not complete while the window was open: %#v", refresh)
	}
	focus := resultFor(&pb.ProviderCommand{
		Context: ctx, Operation: pb.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_FOCUS,
		TargetOperationId: "window-handle",
	}, manager)
	if !focus.Configuration.Accepted {
		t.Fatalf("focus failed: %#v", focus.Configuration)
	}
}

func waitForPreference(manager *configurationWindows, store *fakePreferences) <-chan struct{} {
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			value, _, _ := store.Get(context.Background(), preferenceKey)
			if value == "Library" {
				return
			}
			time.Sleep(time.Millisecond)
		}
	}()
	return done
}
