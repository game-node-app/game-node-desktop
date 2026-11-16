package provider

import (
	"context"
	"reflect"
	"sort"
	"sync"
	"testing"
	"time"

	"example.com/gamenode/plugins/go-grpc/proto"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	pbproto "google.golang.org/protobuf/proto"
)

type observedExchange struct {
	discovery *proto.DiscoveryResult
	chunks    []*proto.CatalogChunk
	result    *proto.OperationResult
	results   map[string]*proto.OperationResult
	contexts  []string
}

type fixtureHost struct {
	proto.UnimplementedPluginHostServer
	proto.UnimplementedProviderHostServer

	credential []byte
	mu         sync.Mutex
	open       *proto.OpenSessionRequest
	context    *proto.ServiceContext
	exchange   observedExchange
}

func (h *fixtureHost) OpenSession(_ context.Context, request *proto.OpenSessionRequest) (*proto.OpenSessionReply, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.open = pbproto.Clone(request).(*proto.OpenSessionRequest)
	if !reflect.DeepEqual(request.GetSessionCredential(), h.credential) {
		return nil, status.Error(codes.Unauthenticated, "fixture credential mismatch")
	}
	return &proto.OpenSessionReply{
		SessionId:           "fixture-session",
		GrantedCapabilities: append([]string(nil), Capabilities...),
		ApiMajor:            APIMajor,
		ApiMinor:            APIMinor,
	}, nil
}

func (h *fixtureHost) GetContext(_ context.Context, request *proto.ServiceContext) (*proto.ProviderContext, error) {
	if request.GetSessionId() != "fixture-session" || request.GetProviderId() != ProviderID ||
		request.GetSessionKind() != proto.SessionKind_SESSION_KIND_PROVIDER ||
		request.GetOperationId() != "fixture-launch" || !reflect.DeepEqual(request.GetSessionCredential(), h.credential) {
		return nil, status.Error(codes.PermissionDenied, "fixture context scope mismatch")
	}
	h.mu.Lock()
	h.context = pbproto.Clone(request).(*proto.ServiceContext)
	h.mu.Unlock()
	return &proto.ProviderContext{
		ProviderId:          ProviderID,
		Platform:            "linux",
		ManifestToml:        "id = \"example-go\"\n",
		MaxRecords:          100,
		MaxMessageBytes:     1024 * 1024,
		CatalogChunkRecords: 100,
	}, nil
}

func (h *fixtureHost) Exchange(stream grpc.BidiStreamingServer[proto.ProviderEvent, proto.ProviderCommand]) error {
	ready, err := stream.Recv()
	if err != nil {
		return err
	}
	if !ready.GetReady() || ready.GetContext().GetSessionId() != "fixture-session" ||
		ready.GetContext().GetProviderId() != ProviderID ||
		!reflect.DeepEqual(ready.GetContext().GetSessionCredential(), h.credential) {
		return status.Error(codes.PermissionDenied, "provider readiness context mismatch")
	}

	commands := []*proto.ProviderCommand{
		fixtureCommand("discover-1", proto.ProviderOperation_PROVIDER_OPERATION_DISCOVER),
		fixtureCommand("catalog-1", proto.ProviderOperation_PROVIDER_OPERATION_SYNC_CATALOG),
		fixtureCommand("counter-1", proto.ProviderOperation_PROVIDER_OPERATION_READ_COUNTER),
	}
	for _, command := range commands {
		if err := stream.Send(command); err != nil {
			return err
		}
		wantChunk := command.GetOperation() == proto.ProviderOperation_PROVIDER_OPERATION_SYNC_CATALOG
		for {
			event, err := stream.Recv()
			if err != nil {
				return err
			}
			if event.GetContext().GetOperationId() != command.GetContext().GetOperationId() ||
				event.GetContext().GetSessionId() != "fixture-session" ||
				event.GetContext().GetProviderId() != ProviderID {
				return status.Error(codes.PermissionDenied, "provider event context mismatch")
			}
			h.mu.Lock()
			h.exchange.contexts = append(h.exchange.contexts, event.GetContext().GetOperationId())
			if chunk := event.GetCatalogChunk(); chunk != nil {
				h.exchange.chunks = append(h.exchange.chunks, pbproto.Clone(chunk).(*proto.CatalogChunk))
				h.mu.Unlock()
				continue
			}
			result := event.GetResult()
			if result == nil {
				h.mu.Unlock()
				return status.Error(codes.InvalidArgument, "expected provider result or catalog chunk")
			}
			copy := pbproto.Clone(result).(*proto.OperationResult)
			h.exchange.result = copy
			if h.exchange.results == nil {
				h.exchange.results = make(map[string]*proto.OperationResult)
			}
			h.exchange.results[command.GetContext().GetOperationId()] = copy
			if copy.GetDiscovery() != nil {
				h.exchange.discovery = pbproto.Clone(copy.GetDiscovery()).(*proto.DiscoveryResult)
			}
			h.mu.Unlock()
			if wantChunk && len(h.exchange.chunks) == 0 {
				return status.Error(codes.InvalidArgument, "catalog result omitted its chunk")
			}
			break
		}
	}
	shutdown := fixtureCommand("shutdown-1", proto.ProviderOperation_PROVIDER_OPERATION_SHUTDOWN)
	return stream.Send(shutdown)
}

func fixtureCommand(operationID string, operation proto.ProviderOperation) *proto.ProviderCommand {
	return &proto.ProviderCommand{
		Context: &proto.ServiceContext{
			SessionId:         "fixture-session",
			SessionCredential: []byte("0123456789abcdef0123456789abcdef"),
			ProviderId:        ProviderID,
			OperationId:       operationID,
			AccountEpoch:      1,
			ProfileEpoch:      1,
			SessionKind:       proto.SessionKind_SESSION_KIND_PROVIDER,
		},
		Operation: operation,
	}
}

func TestProviderExchangeOverLocalTransport(t *testing.T) {
	listener, endpoint := listenFixture(t)
	host := &fixtureHost{credential: []byte("0123456789abcdef0123456789abcdef")}
	server := grpc.NewServer()
	proto.RegisterPluginHostServer(server, host)
	proto.RegisterProviderHostServer(server, host)
	serveDone := make(chan error, 1)
	go func() { serveDone <- server.Serve(listener) }()
	t.Cleanup(func() {
		server.Stop()
		_ = listener.Close()
		select {
		case <-serveDone:
		case <-time.After(time.Second):
			t.Error("fixture gRPC server did not stop")
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	client, err := Open(ctx, endpoint, host.credential, "fixture-launch")
	if err != nil {
		t.Fatalf("open provider: %v", err)
	}
	if got := client.ProviderContext().GetProviderId(); got != ProviderID {
		t.Fatalf("provider context id = %q, want %q", got, ProviderID)
	}
	if got := client.Context().GetSessionId(); got != "fixture-session" {
		t.Fatalf("session id = %q", got)
	}
	if err := client.Run(ctx, Demo{}); err != nil {
		t.Fatalf("run provider: %v", err)
	}

	host.mu.Lock()
	defer host.mu.Unlock()
	if host.open == nil || host.open.GetPluginId() != PluginID || host.open.GetSessionKind() != proto.SessionKind_SESSION_KIND_PROVIDER {
		t.Fatal("provider session did not use the expected identity and kind")
	}
	if host.open.GetMinApiMajor() != APIMajor || host.open.GetMinApiMinor() != APIMinor ||
		host.open.GetMaxApiMajor() != APIMajor || host.open.GetMaxApiMinor() != APIMinor {
		t.Fatalf("unexpected negotiated API range: %d.%d–%d.%d", host.open.GetMinApiMajor(), host.open.GetMinApiMinor(), host.open.GetMaxApiMajor(), host.open.GetMaxApiMinor())
	}
	gotCapabilities := append([]string(nil), host.open.GetRequestedCapabilities()...)
	wantCapabilities := append([]string(nil), Capabilities...)
	sort.Strings(gotCapabilities)
	sort.Strings(wantCapabilities)
	if !reflect.DeepEqual(gotCapabilities, wantCapabilities) {
		t.Fatalf("requested capabilities = %v, want %v", gotCapabilities, wantCapabilities)
	}
	if host.context == nil || host.context.GetOperationId() != "fixture-launch" {
		t.Fatal("provider did not read the host-issued provider context")
	}
	if host.exchange.discovery == nil || host.exchange.discovery.GetProviderId() != ProviderID ||
		len(host.exchange.discovery.GetRecords()) != 1 || host.exchange.discovery.GetRecords()[0].GetId() != "demo-installed-game" {
		t.Fatalf("unexpected discovery result: %v", host.exchange.discovery)
	}
	if len(host.exchange.chunks) != 1 || len(host.exchange.chunks[0].GetGames()) != 1 ||
		host.exchange.chunks[0].GetGames()[0].GetId() != "demo-catalog-game" {
		t.Fatalf("unexpected catalog exchange: %v", host.exchange.chunks)
	}
	catalogResult := host.exchange.results["catalog-1"]
	if catalogResult == nil || len(catalogResult.GetCatalogs()) != 1 {
		t.Fatalf("catalog completion missing: %v", catalogResult)
	}
	completion, ok := catalogResult.GetCatalogs()[0].GetOutcome().(*proto.CatalogCompletion_CompletedRecords)
	if !ok || completion.CompletedRecords != 1 {
		t.Fatalf("catalog completion outcome = %T, want one completed record", catalogResult.GetCatalogs()[0].GetOutcome())
	}
	// The fixture keeps the last operation result; the test host also captures each operation in
	// the result stream below, so use operation ids to ensure all three responses were correlated.
	if !reflect.DeepEqual(host.exchange.contexts, []string{"discover-1", "catalog-1", "catalog-1", "counter-1"}) {
		t.Fatalf("event contexts = %v", host.exchange.contexts)
	}
	counter := host.exchange.results["counter-1"]
	if counter == nil || len(counter.GetCounters()) != 1 || counter.GetCounters()[0].GetId() != "demo-catalog-game" ||
		counter.GetCounters()[0].GetSeconds() != 7200 || counter.GetCounters()[0].LastPlayed == nil {
		t.Fatalf("unexpected counter result: %v", counter)
	}
}
