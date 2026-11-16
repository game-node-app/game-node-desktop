package provider

import (
	"context"
	"errors"
	"fmt"
	"io"

	"example.com/gamenode/plugins/go-grpc/proto"
	"google.golang.org/grpc"
	pbproto "google.golang.org/protobuf/proto"
)

const (
	PluginID   = "org.example.gamenode.go-grpc"
	ProviderID = "example-go"
	APIMajor   = 1
	APIMinor   = 6
)

var Capabilities = []string{
	"provider.context.read",
	"library.discovery",
	"library.catalog",
	"playtime.counter",
}

var ErrNegotiation = errors.New("host did not grant the requested provider API")

// Client owns one authenticated provider session. It contains no host-side authority beyond the
// capability grants returned by OpenSession.
type Client struct {
	conn     *grpc.ClientConn
	context  *proto.ServiceContext
	info     *proto.ProviderContext
	commands grpc.BidiStreamingClient[proto.ProviderEvent, proto.ProviderCommand]
}

// Open authenticates, opens the exchange stream, sends Ready and reads the provider's host context.
func Open(ctx context.Context, endpoint string, credential []byte, operationID string) (*Client, error) {
	if len(credential) != 32 || operationID == "" {
		return nil, errors.New("invalid launch configuration")
	}
	conn, err := dial(ctx, endpoint)
	if err != nil {
		return nil, fmt.Errorf("connect to local host: %w", err)
	}
	fail := func(err error) (*Client, error) {
		_ = conn.Close()
		return nil, err
	}
	pluginClient := proto.NewPluginHostClient(conn)
	providerClient := proto.NewProviderHostClient(conn)
	reply, err := pluginClient.OpenSession(ctx, &proto.OpenSessionRequest{
		PluginId:              PluginID,
		PluginVersion:         "0.1.0",
		ApiMajor:              APIMajor,
		ApiMinor:              APIMinor,
		MinApiMajor:           APIMajor,
		MinApiMinor:           APIMinor,
		MaxApiMajor:           APIMajor,
		MaxApiMinor:           APIMinor,
		SessionCredential:     append([]byte(nil), credential...),
		RequestedCapabilities: append([]string(nil), Capabilities...),
		SessionKind:           proto.SessionKind_SESSION_KIND_PROVIDER,
	})
	if err != nil {
		return fail(fmt.Errorf("open provider session: %w", err))
	}
	if reply.GetApiMajor() != APIMajor || reply.GetApiMinor() < APIMinor {
		return fail(ErrNegotiation)
	}
	for _, requested := range Capabilities {
		if !contains(reply.GetGrantedCapabilities(), requested) {
			return fail(ErrNegotiation)
		}
	}
	serviceContext := &proto.ServiceContext{
		SessionId:         reply.GetSessionId(),
		SessionCredential: append([]byte(nil), credential...),
		ProviderId:        ProviderID,
		OperationId:       operationID,
		SessionKind:       proto.SessionKind_SESSION_KIND_PROVIDER,
	}
	if serviceContext.GetSessionId() == "" {
		return fail(ErrNegotiation)
	}
	commands, err := providerClient.Exchange(ctx)
	if err != nil {
		return fail(fmt.Errorf("open provider exchange: %w", err))
	}
	if err := commands.Send(&proto.ProviderEvent{
		Context: serviceContext,
		Event:   &proto.ProviderEvent_Ready{Ready: true},
	}); err != nil {
		return fail(fmt.Errorf("announce provider readiness: %w", err))
	}
	info, err := providerClient.GetContext(ctx, serviceContext)
	if err != nil {
		_ = commands.CloseSend()
		return fail(fmt.Errorf("read provider context: %w", err))
	}
	if info.GetProviderId() != ProviderID {
		_ = commands.CloseSend()
		return fail(ErrNegotiation)
	}
	return &Client{conn: conn, context: serviceContext, info: info, commands: commands}, nil
}

func contains(values []string, wanted string) bool {
	for _, value := range values {
		if value == wanted {
			return true
		}
	}
	return false
}

func (c *Client) Context() *proto.ServiceContext {
	return pbproto.Clone(c.context).(*proto.ServiceContext)
}

func (c *Client) ProviderContext() *proto.ProviderContext {
	return pbproto.Clone(c.info).(*proto.ProviderContext)
}

// Run answers bounded provider commands until the host closes the stream or sends Shutdown.
func (c *Client) Run(ctx context.Context, implementation Implementation) error {
	defer c.Close()
	for {
		command, err := c.commands.Recv()
		if err == io.EOF || ctx.Err() != nil {
			return nil
		}
		if err != nil {
			return fmt.Errorf("receive provider command: %w", err)
		}
		if command.GetOperation() == proto.ProviderOperation_PROVIDER_OPERATION_SHUTDOWN {
			return nil
		}
		for _, event := range implementation.Handle(command) {
			if err := c.commands.Send(event); err != nil {
				return fmt.Errorf("send provider result: %w", err)
			}
		}
	}
}

func (c *Client) Close() error {
	if c.commands != nil {
		_ = c.commands.CloseSend()
	}
	if c.conn != nil {
		return c.conn.Close()
	}
	return nil
}
