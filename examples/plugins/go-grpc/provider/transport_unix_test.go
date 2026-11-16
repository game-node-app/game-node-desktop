//go:build !windows

package provider

import (
	"net"
	"path/filepath"
	"testing"
)

func listenFixture(t *testing.T) (net.Listener, string) {
	t.Helper()
	endpoint := filepath.Join(t.TempDir(), "plugin.sock")
	listener, err := net.Listen("unix", endpoint)
	if err != nil {
		t.Fatalf("listen on Unix socket: %v", err)
	}
	return listener, endpoint
}
