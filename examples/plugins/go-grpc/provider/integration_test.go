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
	proto.UnimplementedResourceHostServer

	credential           []byte
	mu                   sync.Mutex
	open                 *proto.OpenSessionRequest
	context              *proto.ServiceContext
	exchange             observedExchange
	configuration        bool
	preference           []byte
	preferenceRevision   uint64
	preferenceDurability proto.StorageDurability
	accounts             map[uint64]*proto.AccountIdentity
	secrets              map[uint64][]byte
	grants               map[uint64]map[string]bool
}

func (h *fixtureHost) authContext(ctx *proto.ServiceContext, permission string) error {
	if ctx.GetSessionId() != "fixture-session" || ctx.GetProviderId() != ProviderID ||
		ctx.GetSessionKind() != proto.SessionKind_SESSION_KIND_PROVIDER ||
		!reflect.DeepEqual(ctx.GetSessionCredential(), h.credential) || ctx.GetProfileEpoch() == 0 {
		return status.Error(codes.Unauthenticated, "fixture auth context mismatch")
	}
	if !h.grants[ctx.GetProfileEpoch()][permission] {
		return status.Error(codes.PermissionDenied, "fixture grant denied")
	}
	return nil
}

func (h *fixtureHost) Account(_ context.Context, request *proto.AccountRequest) (*proto.AccountReply, error) {
	if request.GetContext() == nil {
		return nil, status.Error(codes.Unauthenticated, "missing context")
	}
	ctx := request.GetContext()
	h.mu.Lock()
	defer h.mu.Unlock()
	switch action := request.GetAction().(type) {
	case *proto.AccountRequest_Read:
		if err := h.authContext(ctx, "account.state.read"); err != nil {
			return nil, err
		}
		account := h.accounts[ctx.GetProfileEpoch()]
		if account != nil {
			account = pbproto.Clone(account).(*proto.AccountIdentity)
		}
		return &proto.AccountReply{Account: account, Accepted: true}, nil
	case *proto.AccountRequest_Save:
		if err := h.authContext(ctx, "account.state.publish"); err != nil {
			return nil, err
		}
		if action.Save.GetProviderId() != ProviderID {
			return nil, status.Error(codes.InvalidArgument, "unexpected provider")
		}
		h.accounts[ctx.GetProfileEpoch()] = pbproto.Clone(action.Save).(*proto.AccountIdentity)
		return &proto.AccountReply{Accepted: true}, nil
	case *proto.AccountRequest_Clear:
		if err := h.authContext(ctx, "account.state.publish"); err != nil {
			return nil, err
		}
		delete(h.accounts, ctx.GetProfileEpoch())
		return &proto.AccountReply{Accepted: true}, nil
	default:
		return nil, status.Error(codes.InvalidArgument, "missing account action")
	}
}

func (h *fixtureHost) Secret(_ context.Context, request *proto.SecretRequest) (*proto.SecretReply, error) {
	if request.GetContext() == nil {
		return nil, status.Error(codes.Unauthenticated, "missing context")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	ctx := request.GetContext()
	permission := map[proto.SecretOperation]string{
		proto.SecretOperation_SECRET_OPERATION_READ:   "secrets.read",
		proto.SecretOperation_SECRET_OPERATION_WRITE:  "secrets.write",
		proto.SecretOperation_SECRET_OPERATION_DELETE: "secrets.delete",
	}[request.GetOperation()]
	if err := h.authContext(ctx, permission); err != nil {
		return nil, err
	}
	switch request.GetOperation() {
	case proto.SecretOperation_SECRET_OPERATION_WRITE:
		if request.GetKey() != "session" || string(request.GetValue()) != "fixture-token" {
			return nil, status.Error(codes.InvalidArgument, "invalid fixture secret")
		}
		h.secrets[ctx.GetProfileEpoch()] = append([]byte(nil), request.GetValue()...)
		return &proto.SecretReply{Outcome: &proto.SecretReply_Accepted{Accepted: true}}, nil
	case proto.SecretOperation_SECRET_OPERATION_READ:
		value, ok := h.secrets[ctx.GetProfileEpoch()]
		if !ok {
			return &proto.SecretReply{Outcome: &proto.SecretReply_Absent{Absent: true}}, nil
		}
		return &proto.SecretReply{Outcome: &proto.SecretReply_Value{Value: append([]byte(nil), value...)}}, nil
	case proto.SecretOperation_SECRET_OPERATION_DELETE:
		delete(h.secrets, ctx.GetProfileEpoch())
		return &proto.SecretReply{Outcome: &proto.SecretReply_Accepted{Accepted: true}}, nil
	default:
		return nil, status.Error(codes.InvalidArgument, "invalid secret operation")
	}
}

func (h *fixtureHost) Login(_ context.Context, request *proto.LoginRequest) (*proto.LoginReply, error) {
	if request.GetContext() == nil {
		return nil, status.Error(codes.Unauthenticated, "missing context")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if err := h.authContext(request.GetContext(), "auth.webview"); err != nil {
		return nil, err
	}
	if request.GetHandle() != "host-defined-fixture-login" || request.GetOperation() != proto.LoginOperation_LOGIN_OPERATION_OPEN {
		return nil, status.Error(codes.PermissionDenied, "login handle is not in host fixture policy")
	}
	pageResult := `{"authenticated":true}`
	return &proto.LoginReply{Handle: request.GetHandle(), State: proto.LoginState_LOGIN_STATE_OPEN,
		PageResultJson: &pageResult}, nil
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
	if h.configuration {
		commands = append(commands,
			fixtureConfigurationCommand("config-open-1", "window-1", proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_OPEN),
			fixtureConfigurationCommand("config-focus-1", "window-1", proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_FOCUS),
			fixtureConfigurationCommand("config-open-2", "window-2", proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_OPEN),
			fixtureConfigurationCommand("config-close-2", "window-2", proto.ProviderOperation_PROVIDER_OPERATION_CONFIGURATION_CLOSE),
		)
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

func fixtureConfigurationCommand(operationID, windowID string, operation proto.ProviderOperation) *proto.ProviderCommand {
	command := fixtureCommand(operationID, operation)
	command.TargetOperationId = windowID
	return command
}

func (h *fixtureHost) StorageGet(_ context.Context, request *proto.StorageGetRequest) (*proto.StorageGetReply, error) {
	if request.GetContext().GetSessionId() != "fixture-session" || request.GetContext().GetProviderId() != ProviderID ||
		request.GetContext().GetOperationId() != "fixture-launch" || request.GetKey() != preferenceKey {
		return nil, status.Error(codes.PermissionDenied, "fixture storage read scope mismatch")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	return &proto.StorageGetReply{
		Value:      append([]byte(nil), h.preference...),
		Revision:   h.preferenceRevision,
		Found:      h.preferenceRevision > 0,
		Durability: h.preferenceDurability,
	}, nil
}

func (h *fixtureHost) StoragePut(_ context.Context, request *proto.StoragePutRequest) (*proto.StorageMutationReply, error) {
	mutation := request.GetMutation()
	if mutation.GetContext().GetSessionId() != "fixture-session" || mutation.GetContext().GetProviderId() != ProviderID ||
		mutation.GetContext().GetOperationId() != "fixture-launch" || mutation.GetIdempotencyKey() == "" || request.GetKey() != preferenceKey ||
		request.GetDurability() != proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT {
		return nil, status.Error(codes.PermissionDenied, "fixture storage write scope mismatch")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if mutation.GetExpectedRevision() != h.preferenceRevision {
		return &proto.StorageMutationReply{
			Revision:   h.preferenceRevision,
			Outcome:    proto.MutationOutcome_MUTATION_OUTCOME_REVISION_CONFLICT,
			Durability: h.preferenceDurability,
		}, nil
	}
	h.preference = append([]byte(nil), request.GetValue()...)
	h.preferenceRevision++
	h.preferenceDurability = request.GetDurability()
	return &proto.StorageMutationReply{
		Revision:   h.preferenceRevision,
		Outcome:    proto.MutationOutcome_MUTATION_OUTCOME_APPLIED,
		Durability: h.preferenceDurability,
	}, nil
}

type fixtureConfigWindow struct {
	index int
	mu    sync.Mutex
	exit  WindowExit
	done  chan struct{}
	once  sync.Once
}

func (w *fixtureConfigWindow) Focus(context.Context) bool {
	if w.index == 0 {
		w.finish(WindowExit{Preference: []byte("Library"), Saved: true})
	}
	return true
}

func (w *fixtureConfigWindow) Stop(context.Context) error {
	w.finish(WindowExit{})
	return nil
}

func (w *fixtureConfigWindow) Done() <-chan struct{} { return w.done }

func (w *fixtureConfigWindow) Result() WindowExit {
	w.mu.Lock()
	defer w.mu.Unlock()
	return w.exit
}

func (w *fixtureConfigWindow) finish(exit WindowExit) {
	w.once.Do(func() {
		w.mu.Lock()
		w.exit = exit
		w.mu.Unlock()
		close(w.done)
	})
}

type fixtureWindowLauncher struct {
	mu          sync.Mutex
	preferences []string
	windows     []*fixtureConfigWindow
}

func (l *fixtureWindowLauncher) Start(preference string) (ConfigWindow, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	window := &fixtureConfigWindow{index: len(l.windows), done: make(chan struct{})}
	l.preferences = append(l.preferences, preference)
	l.windows = append(l.windows, window)
	return window, nil
}

func TestProviderExchangeOverLocalTransport(t *testing.T) {
	listener, endpoint := listenFixture(t)
	host := &fixtureHost{credential: []byte("0123456789abcdef0123456789abcdef")}
	server := grpc.NewServer()
	proto.RegisterPluginHostServer(server, host)
	proto.RegisterProviderHostServer(server, host)
	proto.RegisterResourceHostServer(server, host)
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

func TestConfigurationLifecyclePersistsPreferenceThroughHostFixture(t *testing.T) {
	listener, endpoint := listenFixture(t)
	host := &fixtureHost{
		credential:    []byte("0123456789abcdef0123456789abcdef"),
		configuration: true,
	}
	server := grpc.NewServer()
	proto.RegisterPluginHostServer(server, host)
	proto.RegisterProviderHostServer(server, host)
	proto.RegisterResourceHostServer(server, host)
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
	launcher := &fixtureWindowLauncher{}
	client.configuration = NewConfigurationLifecycle(
		resourcePreferenceStore{client: proto.NewResourceHostClient(client.conn)},
		launcher,
		client.Context(),
	)
	if err := client.Run(ctx, Demo{}); err != nil {
		t.Fatalf("run provider: %v", err)
	}

	windowIDs := map[string]string{
		"config-open-1":  "window-1",
		"config-focus-1": "window-1",
		"config-open-2":  "window-2",
		"config-close-2": "window-2",
	}
	for id, windowID := range windowIDs {
		result := host.exchange.results[id]
		if result == nil || result.GetConfiguration() == nil || !result.GetConfiguration().GetAccepted() {
			t.Fatalf("configuration operation %s was not accepted: %v", id, result)
		}
		if result.GetConfiguration().GetSessionId() != "fixture-session" {
			t.Fatalf("configuration operation %s lost session correlation: %v", id, result.GetConfiguration())
		}
		if result.GetConfiguration().GetWindowId() != windowID {
			t.Fatalf("configuration operation %s lost window correlation: %v", id, result.GetConfiguration())
		}
	}
	launcher.mu.Lock()
	preferences := append([]string(nil), launcher.preferences...)
	launcher.mu.Unlock()
	if !reflect.DeepEqual(preferences, []string{"Games", "Library"}) {
		t.Fatalf("configuration window preferences = %v, want default then persisted value", preferences)
	}
	host.mu.Lock()
	defer host.mu.Unlock()
	if string(host.preference) != "Library" || host.preferenceRevision != 1 ||
		host.preferenceDurability != proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT {
		t.Fatalf("host did not persist the non-secret preference: value=%q revision=%d durability=%s",
			host.preference, host.preferenceRevision, host.preferenceDurability)
	}
}

func TestAccountSecretAndHostDefinedLoginAreProfileScopedAndGrantChecked(t *testing.T) {
	listener, endpoint := listenFixture(t)
	host := &fixtureHost{
		credential: []byte("0123456789abcdef0123456789abcdef"),
		accounts:   make(map[uint64]*proto.AccountIdentity),
		secrets:    make(map[uint64][]byte),
		grants: map[uint64]map[string]bool{
			1: {"account.state.read": true, "account.state.publish": true, "secrets.read": true, "secrets.write": true, "auth.webview": true},
			2: {"account.state.read": true, "secrets.read": true},
			3: {},
		},
	}
	server := grpc.NewServer()
	proto.RegisterPluginHostServer(server, host)
	proto.RegisterProviderHostServer(server, host)
	proto.RegisterResourceHostServer(server, host)
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
	defer client.Close()
	service := proto.NewProviderHostClient(client.conn)
	profileA := client.Context()
	profileA.ProfileEpoch = 1
	accountName := "Fake User"
	account := &proto.AccountIdentity{ProviderId: ProviderID, AccountId: "fixture-account", AccountName: &accountName}
	if reply, err := service.Account(ctx, &proto.AccountRequest{Context: profileA, Action: &proto.AccountRequest_Save{Save: account}}); err != nil || !reply.GetAccepted() {
		t.Fatalf("save fake account: reply=%v err=%v", reply, err)
	}
	if reply, err := service.Account(ctx, &proto.AccountRequest{Context: profileA, Action: &proto.AccountRequest_Read{Read: true}}); err != nil || reply.GetAccount().GetAccountId() != "fixture-account" {
		t.Fatalf("read fake account: reply=%v err=%v", reply, err)
	}
	if reply, err := service.Secret(ctx, &proto.SecretRequest{Context: profileA, Key: "session", Operation: proto.SecretOperation_SECRET_OPERATION_WRITE, Value: []byte("fixture-token")}); err != nil || !reply.GetAccepted() {
		t.Fatalf("write fake secret: reply=%v err=%v", reply, err)
	}
	if reply, err := service.Secret(ctx, &proto.SecretRequest{Context: profileA, Key: "session", Operation: proto.SecretOperation_SECRET_OPERATION_READ}); err != nil || string(reply.GetValue()) != "fixture-token" {
		t.Fatalf("read fake secret: reply=%v err=%v", reply, err)
	}
	login, err := service.Login(ctx, &proto.LoginRequest{Context: profileA, Operation: proto.LoginOperation_LOGIN_OPERATION_OPEN, Handle: "host-defined-fixture-login"})
	if err != nil || login.GetState() != proto.LoginState_LOGIN_STATE_OPEN || login.GetPageResultJson() != `{"authenticated":true}` || len(login.GetCookies()) != 0 {
		t.Fatalf("host-defined login result = %v, err=%v", login, err)
	}
	if _, err := service.Login(ctx, &proto.LoginRequest{Context: profileA, Operation: proto.LoginOperation_LOGIN_OPERATION_OPEN, Handle: "plugin-supplied-script"}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("plugin-supplied login handle error = %v, want permission denied", err)
	}

	profileB := client.Context()
	profileB.ProfileEpoch = 2
	if reply, err := service.Account(ctx, &proto.AccountRequest{Context: profileB, Action: &proto.AccountRequest_Read{Read: true}}); err != nil || reply.GetAccount() != nil {
		t.Fatalf("account leaked across profiles: reply=%v err=%v", reply, err)
	}
	if reply, err := service.Secret(ctx, &proto.SecretRequest{Context: profileB, Key: "session", Operation: proto.SecretOperation_SECRET_OPERATION_READ}); err != nil || !reply.GetAbsent() {
		t.Fatalf("secret leaked across profiles: reply=%v err=%v", reply, err)
	}
	if _, err := service.Login(ctx, &proto.LoginRequest{Context: profileB, Operation: proto.LoginOperation_LOGIN_OPERATION_OPEN, Handle: "host-defined-fixture-login"}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("login without the profile grant error = %v, want permission denied", err)
	}

	denied := client.Context()
	denied.ProfileEpoch = 3
	if _, err := service.Account(ctx, &proto.AccountRequest{Context: denied, Action: &proto.AccountRequest_Read{Read: true}}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("account read without grant error = %v, want permission denied", err)
	}
	if _, err := service.Secret(ctx, &proto.SecretRequest{Context: denied, Key: "session", Operation: proto.SecretOperation_SECRET_OPERATION_READ}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("secret read without grant error = %v, want permission denied", err)
	}
	if _, err := service.Login(ctx, &proto.LoginRequest{Context: denied, Operation: proto.LoginOperation_LOGIN_OPERATION_OPEN, Handle: "host-defined-fixture-login"}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("login without grant error = %v, want permission denied", err)
	}
}
