//go:build windows

package main

import (
	"context"
	"net"

	"github.com/Microsoft/go-winio"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func dialPlugin(endpoint string) (*grpc.ClientConn, error) {
	return grpc.NewClient("passthrough:///gamenode-plugin",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return winio.DialPipeContext(ctx, endpoint)
		}),
	)
}
