//go:build windows

package provider

import (
	"fmt"
	"net"
	"os"
	"testing"

	"github.com/Microsoft/go-winio"
)

func listenFixture(t *testing.T) (net.Listener, string) {
	t.Helper()
	endpoint := fmt.Sprintf(`\\.\pipe\gamenode-go-example-%d-%d`, os.Getpid(), testPipeSequence.Add(1))
	listener, err := winio.ListenPipe(endpoint, &winio.PipeConfig{})
	if err != nil {
		t.Fatalf("listen on named pipe: %v", err)
	}
	return listener, endpoint
}
