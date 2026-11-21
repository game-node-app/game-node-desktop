package main

// A producer/consumer exchange over a local gRPC fixture host. The fixture models the published
// authorization rules (default-deny common categories, owner-only save profiles, custom-field
// policies, opaque volume tokens) so the Go client runs the same scenario as the Rust example. It
// is an interoperability fixture, not the desktop host: it starts no collector and knows no
// filesystem path.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"sync"
	"testing"

	"example.com/gamenode/plugin-example/proto"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/structpb"
)

const (
	producerCredential = "0123456789abcdef0123456789abcdef"
	consumerCredential = "fedcba9876543210fedcba9876543210"
	saveFolderHandle   = "handle:save-folder"
	saveProfileID      = "profile-fixture"
)

var (
	producerGrants = []string{
		"game.performance.publish",
		"game.performance.read",
		"game.saves.read",
		"game.saves.inspect",
		"game.storage.inspect",
		"data.publish",
	}
	consumerGrants = []string{
		"game.performance.read",
		"game.saves.read",
		"game.runtime.read",
		"library.read",
		"library.read.all",
	}
)

type fixtureSession struct {
	id        string
	pluginID  string
	publisher string
	profile   string
	kind      proto.SessionKind
	target    *proto.TargetGameContext
	grants    map[string]bool
}

func (s *fixtureSession) allows(permission string) bool { return s.grants[permission] }

type fixtureHost struct {
	proto.UnimplementedPluginHostServer
	proto.UnimplementedResourceHostServer
	proto.UnimplementedGamePerformanceServer
	proto.UnimplementedLocationsServer
	proto.UnimplementedSavesServer

	mu       sync.Mutex
	sessions map[string]*fixtureSession

	series    []*proto.PerformanceSeries
	ended     map[string]bool
	category  map[string]map[proto.CommonDataCategory]bool // recipient plugin -> grants
	fields    map[string]map[string]bool                   // recipient plugin + "|" + dataset -> fields
	records   map[string][]*proto.DataRecord
	recordSeq int
}

func newFixtureHost() *fixtureHost {
	return &fixtureHost{
		sessions: map[string]*fixtureSession{
			producerCredential: {
				id:        "producer-session",
				pluginID:  "org.example",
				publisher: "abababababababababababababababababababababababababababababababab",
				profile:   "a",
				kind:      proto.SessionKind_SESSION_KIND_PROVIDER,
				target: &proto.TargetGameContext{
					GameId:         "5",
					InstallationId: "id-steam:1",
					ProfileId:      "a",
					Revision:       "1",
				},
				grants: grantsOf(producerGrants),
			},
			consumerCredential: {
				id:        "consumer-session",
				pluginID:  "org.example.consumer",
				publisher: "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
				profile:   "a",
				kind:      proto.SessionKind_SESSION_KIND_PROVIDER,
				grants:    grantsOf(consumerGrants),
			},
		},
		ended:    map[string]bool{},
		category: map[string]map[proto.CommonDataCategory]bool{},
		fields:   map[string]map[string]bool{},
		records:  map[string][]*proto.DataRecord{},
	}
}

func grantsOf(values []string) map[string]bool {
	grants := make(map[string]bool, len(values))
	for _, value := range values {
		grants[value] = true
	}
	return grants
}

func (h *fixtureHost) session(scope *proto.ServiceContext, permission string) (*fixtureSession, error) {
	if scope == nil {
		return nil, status.Error(codes.Unauthenticated, "resource context required")
	}
	h.mu.Lock()
	session := h.sessions[string(scope.GetSessionCredential())]
	h.mu.Unlock()
	if session == nil || session.id != scope.GetSessionId() || session.pluginID != scope.GetProviderId() {
		return nil, status.Error(codes.Unauthenticated, "unknown fixture session")
	}
	if permission != "" && !session.allows(permission) {
		return nil, status.Error(codes.PermissionDenied, "fixture grant denied")
	}
	return session, nil
}

func (h *fixtureHost) OpenSession(_ context.Context, request *proto.OpenSessionRequest) (*proto.OpenSessionReply, error) {
	credential := string(request.GetSessionCredential())
	h.mu.Lock()
	session := h.sessions[credential]
	h.mu.Unlock()
	if session == nil || request.GetPluginId() != session.pluginID {
		return nil, status.Error(codes.Unauthenticated, "fixture credential mismatch")
	}
	granted := make([]string, 0, len(request.GetRequestedCapabilities()))
	for _, capability := range request.GetRequestedCapabilities() {
		if !session.allows(capability) {
			return nil, status.Error(codes.PermissionDenied, "fixture capability not granted")
		}
		granted = append(granted, capability)
	}
	return &proto.OpenSessionReply{
		SessionId:           session.id,
		GrantedCapabilities: granted,
		ApiMajor:            1,
		ApiMinor:            6,
		ResourceContext: &proto.ServiceContext{
			SessionId:         session.id,
			SessionCredential: append([]byte(nil), request.GetSessionCredential()...),
			ProviderId:        session.pluginID,
			OperationId:       "exchange-launch",
			AccountEpoch:      1,
			ProfileEpoch:      1,
			SessionKind:       session.kind,
			TargetGame:        session.target,
		},
	}, nil
}

// ---- Game performance ----

func (h *fixtureHost) GamePerformancePublishSamples(_ context.Context, request *proto.PerformanceSamplesRequest) (*proto.ReportMutationReply, error) {
	mutation := request.GetMutation()
	session, err := h.session(mutation.GetContext(), "game.performance.publish")
	if err != nil {
		return nil, err
	}
	if session.target == nil || request.GetGameId() != session.target.GetGameId() {
		return nil, status.Error(codes.PermissionDenied, "the selected game does not match")
	}
	if len(request.GetSamples()) == 0 {
		return nil, status.Error(codes.InvalidArgument, "samples are required")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.ended[request.GetSeriesId()] {
		return nil, status.Error(codes.FailedPrecondition, "series already ended")
	}
	var series *proto.PerformanceSeries
	for _, candidate := range h.series {
		if candidate.GetSeriesId() == request.GetSeriesId() && candidate.GetOwnerPluginId() == session.pluginID {
			series = candidate
			break
		}
	}
	if series == nil {
		series = &proto.PerformanceSeries{
			SeriesId:              request.GetSeriesId(),
			GameId:                request.GetGameId(),
			InstallationId:        session.target.GetInstallationId(),
			ComputerContextId:     LocalComputerContext,
			SessionId:             session.id,
			OwnerPluginId:         session.pluginID,
			RuntimeId:             request.GetRuntimeId(),
			RuntimeRecipeRevision: request.GetRuntimeRecipeRevision(),
			Source:                request.GetSource(),
			ToolName:              request.GetToolName(),
			ToolVersion:           request.GetToolVersion(),
			SampleIntervalMs:      request.GetSampleIntervalMs(),
			CpuScope:              request.GetCpuScope(),
			CpuNormalization:      request.GetCpuNormalization(),
			MemoryKind:            request.GetMemoryKind(),
			MemorySource:          request.GetMemorySource(),
			FpsUnit:               request.GetFpsUnit(),
			CpuUnit:               request.GetCpuUnit(),
			MemoryUnit:            request.GetMemoryUnit(),
			Provenance:            "plugin claim; GameNode launched no collector",
		}
		h.series = append(h.series, series)
	}
	if mutation.GetExpectedRevision() != series.GetRevision() {
		return &proto.ReportMutationReply{
			ReportId: series.GetSeriesId(),
			Revision: series.GetRevision(),
			Outcome:  proto.MutationOutcome_MUTATION_OUTCOME_REVISION_CONFLICT,
		}, nil
	}
	for _, sample := range request.GetSamples() {
		if sample.GetSequence() <= series.GetRevision() && len(series.GetSamples()) > 0 &&
			sample.GetSequence() <= series.GetSamples()[len(series.GetSamples())-1].GetSequence() {
			return nil, status.Error(codes.InvalidArgument, "out-of-order sample sequence")
		}
		series.Samples = append(series.Samples, sample)
	}
	series.Revision++
	series.EndedAt = 0
	return &proto.ReportMutationReply{
		ReportId:        series.GetSeriesId(),
		Revision:        series.GetRevision(),
		Outcome:         proto.MutationOutcome_MUTATION_OUTCOME_APPLIED,
		AcceptedSamples: uint32(len(request.GetSamples())),
	}, nil
}

func (h *fixtureHost) GamePerformanceEndSeries(_ context.Context, request *proto.PerformanceEndSeriesRequest) (*proto.MutationReply, error) {
	mutation := request.GetMutation()
	if _, err := h.session(mutation.GetContext(), "game.performance.publish"); err != nil {
		return nil, err
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	index := -1
	for position, candidate := range h.series {
		if candidate.GetSeriesId() == request.GetSeriesId() {
			index = position
			break
		}
	}
	if index < 0 {
		return nil, status.Error(codes.NotFound, "performance series not found")
	}
	if mutation.GetExpectedRevision() != h.series[index].GetRevision() {
		return &proto.MutationReply{
			Revision: h.series[index].GetRevision(),
			Outcome:  proto.MutationOutcome_MUTATION_OUTCOME_REVISION_CONFLICT,
		}, nil
	}
	// Cleanup: the retained history is removed and later batches are refused.
	h.ended[request.GetSeriesId()] = true
	h.series = append(h.series[:index], h.series[index+1:]...)
	return &proto.MutationReply{
		ResourceId: request.GetSeriesId(),
		Outcome:    proto.MutationOutcome_MUTATION_OUTCOME_APPLIED,
	}, nil
}

func (h *fixtureHost) listSeries(session *fixtureSession, request *proto.PerformanceListRequest, gameID string) ([]*proto.PerformanceSeries, error) {
	selector := request.GetPage().GetCommonDataCategories()
	shared := selector != nil && len(selector.GetCategories()) > 0
	h.mu.Lock()
	defer h.mu.Unlock()
	if shared && !h.category[session.pluginID][proto.CommonDataCategory_COMMON_DATA_CATEGORY_GAME_PERFORMANCE_HISTORY] {
		return nil, nil // unauthorized common history is absent, not an error
	}
	var result []*proto.PerformanceSeries
	for _, series := range h.series {
		if gameID != "" && series.GetGameId() != gameID {
			continue
		}
		if !shared && series.GetOwnerPluginId() != session.pluginID {
			continue
		}
		result = append(result, series)
	}
	return result, nil
}

func (h *fixtureHost) List(_ context.Context, request *proto.PerformanceListRequest) (*proto.PerformanceListReply, error) {
	session, err := h.session(request.GetPage().GetContext(), "game.performance.read")
	if err != nil {
		return nil, err
	}
	result, err := h.listSeries(session, request, request.GetGameId())
	if err != nil {
		return nil, err
	}
	return &proto.PerformanceListReply{Series: result, Revision: 1}, nil
}

func (h *fixtureHost) seriesFor(scope *proto.ServiceContext, seriesID string) (*fixtureSession, *proto.PerformanceSeries, error) {
	session, err := h.session(scope, "game.performance.read")
	if err != nil {
		return nil, nil, err
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, series := range h.series {
		if series.GetSeriesId() != seriesID {
			continue
		}
		if series.GetOwnerPluginId() == session.pluginID {
			return session, series, nil
		}
		if h.category[session.pluginID][proto.CommonDataCategory_COMMON_DATA_CATEGORY_GAME_PERFORMANCE_HISTORY] {
			return session, series, nil
		}
	}
	return nil, nil, status.Error(codes.NotFound, "performance series not found")
}

func (h *fixtureHost) QuerySeries(_ context.Context, request *proto.PerformanceQuerySeriesRequest) (*proto.PerformanceQuerySeriesReply, error) {
	_, series, err := h.seriesFor(request.GetContext(), request.GetSeriesId())
	if err != nil {
		return nil, err
	}
	var samples []*proto.PerformanceSample
	for _, sample := range series.GetSamples() {
		if sample.GetObservedAt() >= request.GetFromTime() &&
			(request.GetToTime() == 0 || sample.GetObservedAt() <= request.GetToTime()) {
			samples = append(samples, sample)
		}
	}
	return &proto.PerformanceQuerySeriesReply{Samples: samples, Coverage: "complete"}, nil
}

func (h *fixtureHost) Aggregate(_ context.Context, request *proto.PerformanceAggregateRequest) (*proto.PerformanceAggregateReply, error) {
	_, series, err := h.seriesFor(request.GetContext(), request.GetSeriesId())
	if err != nil {
		return nil, err
	}
	var total float64
	for _, sample := range series.GetSamples() {
		if sample.GetObservedAt() >= request.GetFromTime() &&
			(request.GetToTime() == 0 || sample.GetObservedAt() <= request.GetToTime()) {
			switch request.GetMetric() {
			case "fps":
				total += sample.GetFramesPerSecond()
			case "cpu_percent":
				total += sample.GetCpuPercent()
			case "memory_bytes":
				total += float64(sample.GetMemoryBytes())
			}
		}
	}
	count := uint64(len(series.GetSamples()))
	value := 0.0
	if count > 0 {
		value = total / float64(count)
	}
	units := map[string]string{"fps": series.GetFpsUnit(), "cpu_percent": series.GetCpuUnit(), "memory_bytes": series.GetMemoryUnit()}
	return &proto.PerformanceAggregateReply{
		Metric:      request.GetMetric(),
		Aggregation: request.GetAggregation(),
		Value:       value,
		Unit:        units[request.GetMetric()],
		SampleCount: count,
		Coverage:    "complete",
		Provenance:  series.GetProvenance(),
	}, nil
}

// ---- Locations and saves ----

func (h *fixtureHost) Inspect(_ context.Context, request *proto.LocationInspectRequest) (*proto.LocationInspectReply, error) {
	if _, err := h.session(request.GetContext(), "game.saves.inspect"); err != nil {
		return nil, err
	}
	if request.GetApprovedResourceHandle() != saveFolderHandle {
		return nil, status.Error(codes.NotFound, "approved place not found")
	}
	return &proto.LocationInspectReply{
		Location: &proto.LocationDescription{
			ResourceId:          "res-save-folder",
			Availability:        proto.InspectionAvailability_INSPECTION_AVAILABILITY_AVAILABLE,
			VolumeIdentityToken: "vol-fixture",
			NetworkState:        "local",
			MediaType:           "fixed",
			Attachment:          "internal",
			InterfaceType:       "sata",
			Evidence:            "fixture metadata only",
		},
	}, nil
}

func (h *fixtureHost) GetSize(_ context.Context, request *proto.LocationSizeRequest) (*proto.LocationSizeReply, error) {
	if _, err := h.session(request.GetContext(), "game.saves.inspect"); err != nil {
		return nil, err
	}
	if request.GetApprovedResourceHandle() != saveFolderHandle {
		return nil, status.Error(codes.NotFound, "approved place not found")
	}
	return &proto.LocationSizeReply{
		LogicalBytes:   9,
		FileCount:      2,
		DirectoryCount: 3,
		Complete:       true,
		Availability:   proto.InspectionAvailability_INSPECTION_AVAILABILITY_AVAILABLE,
	}, nil
}

func (h *fixtureHost) ListProfiles(_ context.Context, request *proto.SaveProfileListRequest) (*proto.SaveProfileListReply, error) {
	if _, err := h.session(request.GetContext(), "game.saves.read"); err != nil {
		return nil, err
	}
	if request.GetSharedRecords() != nil && len(request.GetSharedRecords().GetCategories()) > 0 {
		return nil, status.Error(codes.Unimplemented, "save profiles stay owner-only")
	}
	return &proto.SaveProfileListReply{
		Profiles: []*proto.SaveProfile{saveProfile()},
		Revision: 1,
	}, nil
}

func (h *fixtureHost) GetProfile(_ context.Context, request *proto.SaveProfileGetRequest) (*proto.SaveProfileReply, error) {
	session, err := h.session(request.GetContext(), "game.saves.read")
	if err != nil {
		return nil, err
	}
	if session.pluginID != "org.example" || request.GetProfileId() != saveProfileID {
		return nil, status.Error(codes.NotFound, "save profile not found")
	}
	profile := saveProfile()
	return &proto.SaveProfileReply{Profile: profile, Revision: profile.GetRevision()}, nil
}

func saveProfile() *proto.SaveProfile {
	return &proto.SaveProfile{
		ProfileId:      saveProfileID,
		GameId:         "3",
		InstallationId: "id-fixture:1",
		Name:           "Example memory card",
		Directories: []*proto.SaveDirectory{{
			ApprovedResourceHandle: saveFolderHandle,
			DisplayName:            "Memory card",
			NamingPatterns:         []string{"GC/**/*.gci"},
		}},
		RuntimeId:       "org.example.emulator",
		FormatId:        "gci",
		FormatVersion:   2,
		CreatorPluginId: "org.example",
		Revision:        1,
	}
}

// ---- Runtime catalog and content availability ----

func (h *fixtureHost) RuntimesList(_ context.Context, request *proto.DomainListRequest) (*proto.RuntimesListReply, error) {
	session, err := h.session(request.GetPage().GetContext(), "game.runtime.read")
	if err != nil {
		return nil, err
	}
	selector := request.GetPage().GetCommonDataCategories()
	if selector == nil || len(selector.GetCategories()) == 0 {
		return &proto.RuntimesListReply{}, nil
	}
	h.mu.Lock()
	granted := h.category[session.pluginID][proto.CommonDataCategory_COMMON_DATA_CATEGORY_RUNTIME_CATALOG]
	h.mu.Unlock()
	if !granted {
		return nil, status.Error(codes.PermissionDenied, "runtime catalog grant missing")
	}
	extensions := map[string]*proto.ExtensionPayload{}
	// The known namespace is validated by the producer's packaged schema; the future namespace is
	// carried as a bounded opaque envelope that the consumer must not interpret.
	extensions["org.example.runtime"] = extensionPayload("org.example.runtime.v1", 1, 173, map[string]any{"display_family": "desktop"})
	extensions["org.vendor.future-runtime"] = extensionPayload("org.vendor.future-runtime.v9", 9, 254, map[string]any{"new_hint": "preserve without parsing"})
	return &proto.RuntimesListReply{
		Runtimes: []*proto.RuntimeRecord{{
			Id:               "runtime:example-snes",
			Name:             "Example SNES",
			Version:          "2.0",
			ExecutableHandle: "",
			SupportedFormats: []string{"snes-rom"},
			Extensions:       extensions,
			Revision:         2,
			Platform:         "linux",
			Provenance: &proto.DataProvenance{
				ProducerPluginId:     "org.example",
				PublisherFingerprint: "abababababababababababababababababababababababababababababababab",
			},
		}},
		Revision: 2,
	}, nil
}

func extensionPayload(schemaID string, version uint32, digest byte, fields map[string]any) *proto.ExtensionPayload {
	data, err := structpb.NewStruct(fields)
	if err != nil {
		panic(err)
	}
	return &proto.ExtensionPayload{
		SchemaId:      schemaID,
		SchemaVersion: version,
		SchemaDigest:  []byte{digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest, digest},
		Data:          data,
	}
}

func (h *fixtureHost) LibraryGet(_ context.Context, request *proto.LibraryGetRequest) (*proto.LibraryGetReply, error) {
	session, err := h.session(request.GetContext(), "library.read.all")
	if err != nil {
		return nil, err
	}
	entry := &proto.LibraryEntry{
		GameId:         "5",
		InstallationId: request.GetInstallationId(),
		ProviderId:     "steam",
		Name:           "Shared runtime game",
		InstallState:   "installed",
		Revision:       1,
	}
	selector := request.GetCommonDataCategories()
	if selector != nil && len(selector.GetCategories()) > 0 {
		h.mu.Lock()
		granted := h.category[session.pluginID][proto.CommonDataCategory_COMMON_DATA_CATEGORY_CONTENT_AVAILABILITY]
		h.mu.Unlock()
		if !granted {
			return &proto.LibraryGetReply{Entry: entry}, nil
		}
		// Two content identities resolve to the same shared runtime.
		entry.RuntimeAvailability = []*proto.RuntimeAvailability{
			{ContentId: "rom:zelda", ContentKind: "snes-rom", RuntimeId: "runtime:example-snes", RuntimeName: "Example SNES", Platform: "linux", PublisherId: "org.example", Revision: 1},
			{ContentId: "rom:metroid", ContentKind: "snes-rom", RuntimeId: "runtime:example-snes", RuntimeName: "Example SNES", Platform: "linux", PublisherId: "org.example", Revision: 1},
		}
	}
	return &proto.LibraryGetReply{Entry: entry}, nil
}

// ---- Custom and opaque datasets ----

func (h *fixtureHost) DataPublish(_ context.Context, request *proto.DataPublishRequest) (*proto.DataMutationReply, error) {
	session, err := h.session(request.GetMutation().GetContext(), "data.publish")
	if err != nil {
		return nil, err
	}
	if request.GetValue() == nil && len(request.GetOpaquePayload()) == 0 {
		return nil, status.Error(codes.InvalidArgument, "a payload is required")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.recordSeq++
	record := &proto.DataRecord{
		RecordId:            fmt.Sprintf("record-%d", h.recordSeq),
		DatasetId:           request.GetDatasetId(),
		PublisherId:         session.pluginID,
		Revision:            1,
		Value:               request.GetValue(),
		SchemaId:            request.GetSchemaId(),
		SchemaVersion:       request.GetSchemaVersion(),
		OpaquePayload:       append([]byte(nil), request.GetOpaquePayload()...),
		ResourceId:          request.GetResourceId(),
		ObservedAt:          request.GetObservedAt(),
		Sequence:            request.GetSequence(),
		SourceObservationId: request.GetSourceObservationId(),
		Durability:          request.GetDurability(),
		Provenance: &proto.DataProvenance{
			ProducerPluginId:     session.pluginID,
			PublisherFingerprint: session.publisher,
		},
	}
	h.records[request.GetDatasetId()] = append(h.records[request.GetDatasetId()], record)
	return &proto.DataMutationReply{
		RecordId:   record.GetRecordId(),
		Revision:   1,
		Outcome:    proto.MutationOutcome_MUTATION_OUTCOME_APPLIED,
		Durability: record.GetDurability(),
	}, nil
}

func (h *fixtureHost) DataList(_ context.Context, request *proto.DataListRequest) (*proto.DataListReply, error) {
	session, err := h.session(request.GetPage().GetContext(), "")
	if err != nil {
		return nil, err
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	policy := h.fields[session.pluginID+"|"+request.GetDatasetId()]
	if len(policy) == 0 {
		return &proto.DataListReply{Revision: 1}, nil
	}
	var records []*proto.DataRecord
	for _, record := range h.records[request.GetDatasetId()] {
		projected := &proto.DataRecord{
			RecordId:            record.GetRecordId(),
			DatasetId:           record.GetDatasetId(),
			PublisherId:         record.GetPublisherId(),
			Revision:            record.GetRevision(),
			SchemaId:            record.GetSchemaId(),
			SchemaVersion:       record.GetSchemaVersion(),
			ResourceId:          record.GetResourceId(),
			ObservedAt:          record.GetObservedAt(),
			ReceivedAt:          record.GetReceivedAt(),
			ExpiresAt:           record.GetExpiresAt(),
			Sequence:            record.GetSequence(),
			SourceObservationId: record.GetSourceObservationId(),
			Durability:          record.GetDurability(),
			Provenance:          record.GetProvenance(),
		}
		if len(record.GetOpaquePayload()) > 0 {
			if !policy["opaque_payload"] {
				continue
			}
			projected.OpaquePayload = append([]byte(nil), record.GetOpaquePayload()...)
		} else {
			value := &structpb.Struct{Fields: map[string]*structpb.Value{}}
			for field, item := range record.GetValue().GetFields() {
				if policy[field] {
					value.Fields[field] = item
				}
			}
			projected.Value = value
		}
		records = append(records, projected)
	}
	return &proto.DataListReply{Records: records, Revision: 1}, nil
}

func (h *fixtureHost) allow(recipient string, category proto.CommonDataCategory) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.category[recipient] == nil {
		h.category[recipient] = map[proto.CommonDataCategory]bool{}
	}
	h.category[recipient][category] = true
}

func (h *fixtureHost) revoke(recipient string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	delete(h.category, recipient)
}

func (h *fixtureHost) grantFields(recipient, dataset string, fields ...string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	key := recipient + "|" + dataset
	if h.fields[key] == nil {
		h.fields[key] = map[string]bool{}
	}
	for _, field := range fields {
		h.fields[key][field] = true
	}
}

func (h *fixtureHost) revokeFields(recipient, dataset string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	delete(h.fields, recipient+"|"+dataset)
}

// ---- Test plumbing ----

func exchangeContract(t *testing.T) map[string]any {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("locate the test file")
	}
	path := filepath.Join(filepath.Dir(file), "..", "..", "crates", "plugin-protocol", "contracts", "data_exchange_fixtures.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read the shared exchange fixture: %v", err)
	}
	var contract map[string]any
	if err := json.Unmarshal(raw, &contract); err != nil {
		t.Fatalf("parse the shared exchange fixture: %v", err)
	}
	return contract
}

func object(t *testing.T, value any) map[string]any {
	t.Helper()
	result, ok := value.(map[string]any)
	if !ok {
		t.Fatalf("expected an object, got %T", value)
	}
	return result
}

func number(t *testing.T, value any) float64 {
	t.Helper()
	result, ok := value.(float64)
	if !ok {
		t.Fatalf("expected a number, got %T", value)
	}
	return result
}

func text(t *testing.T, value any) string {
	t.Helper()
	result, ok := value.(string)
	if !ok {
		t.Fatalf("expected a string, got %T", value)
	}
	return result
}

func list(t *testing.T, value any) []any {
	t.Helper()
	result, ok := value.([]any)
	if !ok {
		t.Fatalf("expected an array, got %T", value)
	}
	return result
}

func startExchange(t *testing.T) (*fixtureHost, proto.PluginHostClient, proto.ResourceHostClient, proto.GamePerformanceClient, proto.LocationsClient, proto.SavesClient) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen for the fixture host: %v", err)
	}
	host := newFixtureHost()
	server := grpc.NewServer()
	proto.RegisterPluginHostServer(server, host)
	proto.RegisterResourceHostServer(server, host)
	proto.RegisterGamePerformanceServer(server, host)
	proto.RegisterLocationsServer(server, host)
	proto.RegisterSavesServer(server, host)
	served := make(chan error, 1)
	go func() { served <- server.Serve(listener) }()
	t.Cleanup(func() {
		server.Stop()
		_ = listener.Close()
		select {
		case err := <-served:
			if err != nil && !errors.Is(err, grpc.ErrServerStopped) {
				t.Errorf("fixture host stopped with %v", err)
			}
		default:
		}
	})
	connection, err := grpc.NewClient(listener.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("connect to the fixture host: %v", err)
	}
	t.Cleanup(func() { _ = connection.Close() })
	return host,
		proto.NewPluginHostClient(connection),
		proto.NewResourceHostClient(connection),
		proto.NewGamePerformanceClient(connection),
		proto.NewLocationsClient(connection),
		proto.NewSavesClient(connection)
}

func openSession(t *testing.T, client proto.PluginHostClient, credential, pluginID string, grants []string) *proto.ServiceContext {
	t.Helper()
	reply, err := client.OpenSession(context.Background(), &proto.OpenSessionRequest{
		PluginId:              pluginID,
		SessionCredential:     []byte(credential),
		RequestedCapabilities: grants,
		MinApiMajor:           1,
		MinApiMinor:           6,
		MaxApiMajor:           1,
		MaxApiMinor:           6,
		SessionKind:           proto.SessionKind_SESSION_KIND_PROVIDER,
	})
	if err != nil {
		t.Fatalf("open fixture session: %v", err)
	}
	return reply.GetResourceContext()
}

func TestPerformanceExchangeAndExplicitCleanup(t *testing.T) {
	contract := exchangeContract(t)
	performance := object(t, contract["performance"])
	host, plugin, resources, gamePerformance, _, _ := startExchange(t)
	producer := openSession(t, plugin, producerCredential, "org.example", producerGrants)
	consumer := openSession(t, plugin, consumerCredential, "org.example.consumer", consumerGrants)

	samples := []*proto.PerformanceSample{}
	for _, raw := range list(t, performance["samples"]) {
		sample := object(t, raw)
		samples = append(samples, PerformanceSample(
			uint64(number(t, sample["sequence"])),
			int64(number(t, sample["observed_at"])),
			number(t, sample["fps"]),
		))
	}
	batch := PerformanceBatch(
		producer,
		"performance-batch",
		0,
		text(t, performance["series_id"]),
		text(t, performance["game_id"]),
		samples,
	)
	if got, want := batch.GetSampleIntervalMs(), uint32(number(t, performance["sample_interval_ms"])); got != want {
		t.Fatalf("sample interval = %d, want %d", got, want)
	}
	units := object(t, performance["units"])
	if batch.GetFpsUnit() != text(t, units["fps"]) || batch.GetCpuUnit() != text(t, units["cpu"]) || batch.GetMemoryUnit() != text(t, units["memory"]) {
		t.Fatalf("units = %q/%q/%q", batch.GetFpsUnit(), batch.GetCpuUnit(), batch.GetMemoryUnit())
	}
	published, err := PublishPerformance(context.Background(), resources, batch)
	if err != nil {
		t.Fatalf("publish performance: %v", err)
	}
	if published.GetAcceptedSamples() != 2 {
		t.Fatalf("accepted samples = %d, want 2", published.GetAcceptedSamples())
	}

	private, err := ListPerformance(context.Background(), gamePerformance, consumer, text(t, performance["game_id"]), nil, "", 10)
	if err != nil {
		t.Fatalf("list private performance: %v", err)
	}
	if len(private.GetSeries()) != 0 {
		t.Fatalf("private-only list returned %d series", len(private.GetSeries()))
	}
	selected, err := ListPerformance(context.Background(), gamePerformance, consumer, text(t, performance["game_id"]), PerformanceSelector(), "", 10)
	if err != nil {
		t.Fatalf("list selected performance: %v", err)
	}
	if len(selected.GetSeries()) != 0 {
		t.Fatalf("a selector without the grant returned %d series", len(selected.GetSeries()))
	}

	host.allow("org.example.consumer", proto.CommonDataCategory_COMMON_DATA_CATEGORY_GAME_PERFORMANCE_HISTORY)
	granted, err := ListPerformance(context.Background(), gamePerformance, consumer, text(t, performance["game_id"]), PerformanceSelector(), "", 10)
	if err != nil {
		t.Fatalf("list granted performance: %v", err)
	}
	if len(granted.GetSeries()) != 1 {
		t.Fatalf("granted list returned %d series", len(granted.GetSeries()))
	}
	series := granted.GetSeries()[0]
	if series.GetComputerContextId() != text(t, performance["computer_context_id"]) {
		t.Fatalf("computer context = %q", series.GetComputerContextId())
	}
	if series.GetInstallationId() != text(t, performance["installation_id"]) {
		t.Fatalf("installation = %q", series.GetInstallationId())
	}
	if series.GetFpsUnit() != text(t, units["fps"]) {
		t.Fatalf("series fps unit = %q", series.GetFpsUnit())
	}
	if len(series.GetSamples()) != 2 {
		t.Fatalf("series has %d samples", len(series.GetSamples()))
	}

	aggregate, err := AggregatePerformance(context.Background(), gamePerformance, consumer, series.GetSeriesId(), "fps", "average", PerformanceSelector())
	if err != nil {
		t.Fatalf("aggregate performance: %v", err)
	}
	if aggregate.GetValue() != number(t, performance["average_fps"]) {
		t.Fatalf("average fps = %v", aggregate.GetValue())
	}
	if aggregate.GetUnit() != text(t, object(t, performance["metric_units"])["fps"]) {
		t.Fatalf("aggregate unit = %q", aggregate.GetUnit())
	}

	host.revoke("org.example.consumer")
	revoked, err := ListPerformance(context.Background(), gamePerformance, consumer, text(t, performance["game_id"]), PerformanceSelector(), "", 10)
	if err != nil {
		t.Fatalf("list revoked performance: %v", err)
	}
	if len(revoked.GetSeries()) != 0 {
		t.Fatalf("revoked list returned %d series", len(revoked.GetSeries()))
	}

	ended, err := EndPerformance(context.Background(), resources, producer, "performance-end", text(t, performance["series_id"]), published.GetRevision(), 2, samples[1].GetObservedAt())
	if err != nil {
		t.Fatalf("end performance series: %v", err)
	}
	if ended.GetOutcome() != proto.MutationOutcome_MUTATION_OUTCOME_APPLIED {
		t.Fatalf("end outcome = %v", ended.GetOutcome())
	}
	after := PerformanceBatch(producer, "performance-after-end", ended.GetRevision(), text(t, performance["series_id"]), text(t, performance["game_id"]), []*proto.PerformanceSample{PerformanceSample(3, 0, 60)})
	if _, err := PublishPerformance(context.Background(), resources, after); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("publish after end error = %v, want failed precondition", err)
	}
	if _, err := QueryPerformance(context.Background(), gamePerformance, producer, text(t, performance["series_id"]), 0, 0, 0, nil); status.Code(err) != codes.NotFound {
		t.Fatalf("query after cleanup error = %v, want not found", err)
	}
}

func TestInspectionSaveProfilesAndSharedRuntimeIdentities(t *testing.T) {
	contract := exchangeContract(t)
	locations := object(t, contract["locations"])
	saves := object(t, contract["save_profiles"])
	catalog := object(t, contract["runtime_catalog"])
	host, plugin, resources, _, locationClient, savesClient := startExchange(t)
	producer := openSession(t, plugin, producerCredential, "org.example", producerGrants)
	consumer := openSession(t, plugin, consumerCredential, "org.example.consumer", consumerGrants)

	inspected, err := InspectLocation(context.Background(), locationClient, producer, saveFolderHandle)
	if err != nil {
		t.Fatalf("inspect the approved folder: %v", err)
	}
	location := inspected.GetLocation()
	if location.GetVolumeIdentityToken() == "" || location.GetAvailability() != proto.InspectionAvailability_INSPECTION_AVAILABILITY_AVAILABLE {
		t.Fatalf("location = %v", location)
	}
	if location.GetMountPoint() != "" {
		t.Fatalf("inspection disclosed a mount point %q", location.GetMountPoint())
	}
	measured, err := MeasureLocation(context.Background(), locationClient, producer, saveFolderHandle)
	if err != nil {
		t.Fatalf("measure the approved folder: %v", err)
	}
	size := object(t, locations["size"])
	if measured.GetLogicalBytes() != uint64(number(t, size["logical_bytes"])) ||
		measured.GetFileCount() != uint64(number(t, size["file_count"])) ||
		measured.GetDirectoryCount() != uint64(number(t, size["directory_count"])) {
		t.Fatalf("measured = %v", measured)
	}

	if _, err := ListSaveProfiles(context.Background(), savesClient, producer, text(t, saves["installation_id"]), CategorySelector(proto.CommonDataCategory_COMMON_DATA_CATEGORY_SAVE_PROFILES)); status.Code(err) != codes.Unimplemented {
		t.Fatalf("shared save profile error = %v, want unimplemented", err)
	}
	profile, err := ReadSaveProfile(context.Background(), savesClient, producer, "", saveProfileID)
	if err != nil {
		t.Fatalf("read the owner save profile: %v", err)
	}
	if profile.GetProfile().GetName() != text(t, saves["name"]) ||
		profile.GetProfile().GetFormatId() != text(t, saves["format_id"]) ||
		profile.GetProfile().GetDirectories()[0].GetApprovedResourceHandle() != saveFolderHandle {
		t.Fatalf("profile = %v", profile.GetProfile())
	}

	if _, err := ListSharedRuntimes(context.Background(), resources, consumer); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("shared runtime without grant error = %v, want permission denied", err)
	}
	host.allow("org.example.consumer", proto.CommonDataCategory_COMMON_DATA_CATEGORY_RUNTIME_CATALOG)
	host.allow("org.example.consumer", proto.CommonDataCategory_COMMON_DATA_CATEGORY_CONTENT_AVAILABILITY)
	runtimes, err := ListSharedRuntimes(context.Background(), resources, consumer)
	if err != nil {
		t.Fatalf("list shared runtimes: %v", err)
	}
	if len(runtimes.GetRuntimes()) != 1 {
		t.Fatalf("shared runtimes = %d", len(runtimes.GetRuntimes()))
	}
	runtime := runtimes.GetRuntimes()[0]
	if runtime.GetId() != text(t, catalog["runtime_id"]) || runtime.GetExecutableHandle() != text(t, catalog["executable_handle_in_shared_projection"]) {
		t.Fatalf("shared runtime = %v", runtime)
	}
	if runtime.GetProvenance().GetProducerPluginId() != "org.example" {
		t.Fatalf("shared runtime provenance = %v", runtime.GetProvenance())
	}
	extension := object(t, contract["extension"])
	if runtime.GetExtensions()["org.example.runtime"].GetSchemaId() != text(t, extension["known_schema_id"]) ||
		runtime.GetExtensions()["org.vendor.future-runtime"].GetSchemaVersion() != uint32(number(t, extension["future_schema_version"])) {
		t.Fatalf("shared runtime extensions = %v", runtime.GetExtensions())
	}

	availability, err := ContentAvailability(context.Background(), resources, consumer, text(t, catalog["installation_id"]))
	if err != nil {
		t.Fatalf("read content availability: %v", err)
	}
	rows := availability.GetEntry().GetRuntimeAvailability()
	if len(rows) != 2 {
		t.Fatalf("content availability rows = %d", len(rows))
	}
	var contentIDs []string
	for _, row := range rows {
		if row.GetRuntimeId() != text(t, catalog["runtime_id"]) || row.GetContentKind() != text(t, catalog["content_kind"]) {
			t.Fatalf("content row = %v", row)
		}
		contentIDs = append(contentIDs, row.GetContentId())
	}
	sort.Strings(contentIDs)
	if !reflect.DeepEqual(contentIDs, []string{"rom:metroid", "rom:zelda"}) {
		t.Fatalf("content identities = %v", contentIDs)
	}

	host.revoke("org.example.consumer")
	if _, err := ListSharedRuntimes(context.Background(), resources, consumer); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("revoked runtime access error = %v, want permission denied", err)
	}
}

func TestCustomStructAndOpaqueEnvelopesProjectGrantedFields(t *testing.T) {
	contract := exchangeContract(t)
	extension := object(t, contract["extension"])
	host, plugin, resources, _, _, _ := startExchange(t)
	producer := openSession(t, plugin, producerCredential, "org.example", producerGrants)
	consumer := openSession(t, plugin, consumerCredential, "org.example.consumer", consumerGrants)

	customFields := object(t, extension["custom_fields"])
	value, err := structpb.NewStruct(customFields)
	if err != nil {
		t.Fatalf("build the custom struct: %v", err)
	}
	published, err := PublishCustom(context.Background(), resources, CustomDatasetBatch(
		producer,
		"custom-notes",
		text(t, extension["custom_dataset_id"]),
		text(t, extension["custom_schema_id"]),
		uint32(number(t, extension["custom_schema_version"])),
		value,
		1,
		"custom-observation-1",
	))
	if err != nil {
		t.Fatalf("publish the custom dataset: %v", err)
	}
	if published.GetOutcome() != proto.MutationOutcome_MUTATION_OUTCOME_APPLIED {
		t.Fatalf("custom publish outcome = %v", published.GetOutcome())
	}
	without, err := ListCustom(context.Background(), resources, consumer, text(t, extension["custom_dataset_id"]))
	if err != nil {
		t.Fatalf("list without a custom grant: %v", err)
	}
	if len(without.GetRecords()) != 0 {
		t.Fatalf("a custom record was returned without a policy")
	}
	projectedField := text(t, list(t, extension["projection_paths"])[0])
	host.grantFields("org.example.consumer", text(t, extension["custom_dataset_id"]), projectedField)
	projected, err := ListCustom(context.Background(), resources, consumer, text(t, extension["custom_dataset_id"]))
	if err != nil {
		t.Fatalf("list the projected custom dataset: %v", err)
	}
	if len(projected.GetRecords()) != 1 {
		t.Fatalf("projected records = %d", len(projected.GetRecords()))
	}
	fields := projected.GetRecords()[0].GetValue().GetFields()
	if len(fields) != 1 || fields[projectedField] == nil {
		t.Fatalf("projected fields = %v", fields)
	}
	if _, ok := fields[customFields["note"].(string)]; ok {
		t.Fatalf("an ungranted field was returned")
	}

	opaque := OpaqueDatasetBatch(producer, "custom-notes-binary", text(t, extension["opaque_dataset_id"]), text(t, extension["opaque_schema_id"]), []byte{0, 1, 2, 255}, 1, "custom-binary-1")
	if _, err := PublishCustom(context.Background(), resources, opaque); err != nil {
		t.Fatalf("publish the opaque dataset: %v", err)
	}
	opaqueWithout, err := ListCustom(context.Background(), resources, consumer, text(t, extension["opaque_dataset_id"]))
	if err != nil {
		t.Fatalf("list the opaque dataset without a grant: %v", err)
	}
	if len(opaqueWithout.GetRecords()) != 0 {
		t.Fatalf("opaque bytes were returned without a grant")
	}
	host.grantFields("org.example.consumer", text(t, extension["opaque_dataset_id"]), text(t, extension["opaque_grant_field"]))
	opaqueGranted, err := ListCustom(context.Background(), resources, consumer, text(t, extension["opaque_dataset_id"]))
	if err != nil {
		t.Fatalf("list the granted opaque dataset: %v", err)
	}
	if len(opaqueGranted.GetRecords()) != 1 || !reflect.DeepEqual(opaqueGranted.GetRecords()[0].GetOpaquePayload(), []byte{0, 1, 2, 255}) {
		t.Fatalf("opaque record = %v", opaqueGranted.GetRecords())
	}
	if len(opaqueGranted.GetRecords()[0].GetValue().GetFields()) != 0 {
		t.Fatalf("opaque bytes invented Struct fields")
	}

	host.revokeFields("org.example.consumer", text(t, extension["custom_dataset_id"]))
	revoked, err := ListCustom(context.Background(), resources, consumer, text(t, extension["custom_dataset_id"]))
	if err != nil {
		t.Fatalf("list the revoked custom dataset: %v", err)
	}
	if len(revoked.GetRecords()) != 0 {
		t.Fatalf("a revoked custom grant still returned records")
	}
}
