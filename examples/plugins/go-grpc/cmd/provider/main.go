package main

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"os/signal"
	"strings"
	"syscall"

	"example.com/gamenode/plugins/go-grpc/provider"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "provider stopped:", err)
		os.Exit(1)
	}
}

func run() error {
	endpoint := strings.TrimSpace(os.Getenv("GAMENODE_PLUGIN_ENDPOINT"))
	credentialText := strings.TrimSpace(os.Getenv("GAMENODE_PLUGIN_SESSION_CREDENTIAL"))
	operationID := strings.TrimSpace(os.Getenv("GAMENODE_PLUGIN_OPERATION_ID"))
	credential, err := hex.DecodeString(credentialText)
	if err != nil || len(credential) != 32 || endpoint == "" || operationID == "" {
		return errors.New("GAMENODE_PLUGIN_ENDPOINT, a 32-byte hex GAMENODE_PLUGIN_SESSION_CREDENTIAL, and GAMENODE_PLUGIN_OPERATION_ID are required")
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	client, err := provider.Open(ctx, endpoint, credential, operationID)
	if err != nil {
		return err
	}
	defer client.Close()
	return client.Run(ctx, provider.Demo{})
}
