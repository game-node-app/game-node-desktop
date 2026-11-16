//go:build !windows

package provider

import (
	"context"
	"net"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func dial(ctx context.Context, endpoint string) (*grpc.ClientConn, error) {
	return grpc.DialContext(ctx, "passthrough:///gamenode-local", grpc.WithBlock(),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return (&net.Dialer{}).DialContext(ctx, "unix", endpoint)
		}),
	)
}
