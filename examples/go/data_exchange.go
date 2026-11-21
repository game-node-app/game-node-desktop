// Producer/consumer exchanges for performance history, approved locations, save profiles, shared
// runtimes and custom datasets.
//
// The functions build the exact generated messages and send them through the generated clients; no
// collector starts and no filesystem path is ever received. Values the host interprets: the
// selected game/installation and `active-host` computer context come from the authenticated
// session, units/sequence are validated, an approved folder is named only by an opaque handle and
// a shared runtime projects an empty executable handle. Source/tool names are descriptive.
package main

import (
	"context"

	"example.com/gamenode/plugin-example/proto"
	"google.golang.org/protobuf/types/known/structpb"
)

const (
	// LocalComputerContext is the only computer identity the host binds to a session.
	LocalComputerContext = "active-host"
	// PerformanceProjection selects the host-defined retained-series projection.
	PerformanceProjection = "performance.v1"
)

// LocationDimensions are the inspection dimensions this build serves. A folder is inspected by an
// opaque handle and answered with an opaque volume token; no field carries its path.
var LocationDimensions = []string{"availability", "volume", "capacity"}

// CategorySelector selects exactly one common-data category for a query, default-deny style.
func CategorySelector(category proto.CommonDataCategory) *proto.CommonDataSelector {
	return &proto.CommonDataSelector{Categories: []proto.CommonDataCategory{category}}
}

// PerformanceSelector selects the common performance-history category. Without it a query is
// private-only; with it the current grant decides whether another plugin's series is visible.
func PerformanceSelector() *proto.CommonDataSelector {
	return CategorySelector(proto.CommonDataCategory_COMMON_DATA_CATEGORY_GAME_PERFORMANCE_HISTORY)
}

// PerformanceSample is one actual observation with explicit units and measurement semantics.
// GameNode does not capture it; the publisher owns collection and the values are a claim.
func PerformanceSample(sequence uint64, observedAt int64, fps float64) *proto.PerformanceSample {
	return &proto.PerformanceSample{
		ObservedAt:       observedAt,
		FramesPerSecond:  fps,
		CpuPercent:       34.5,
		MemoryBytes:      512 * 1024 * 1024,
		Sequence:         sequence,
		SamplingWindowMs: 0,
		FpsMethod:        proto.PerformanceFpsMethod_PERFORMANCE_FPS_METHOD_INSTANTANEOUS,
		FpsStatistic:     proto.PerformanceFpsStatistic_PERFORMANCE_FPS_STATISTIC_INSTANTANEOUS,
		Quality:          proto.PerformanceMeasurementQuality_PERFORMANCE_MEASUREMENT_QUALITY_MEASURED,
	}
}

// PerformanceBatch builds one bounded batch for the selected game. `expectedRevision` is the
// series revision from the previous batch, 0 for the first.
func PerformanceBatch(scope *proto.ServiceContext, key string, expectedRevision uint64, seriesID, gameID string, samples []*proto.PerformanceSample) *proto.PerformanceSamplesRequest {
	return &proto.PerformanceSamplesRequest{
		Mutation: &proto.MutationContext{
			Context:          scope,
			IdempotencyKey:   key,
			ExpectedRevision: expectedRevision,
		},
		SeriesId:              seriesID,
		GameId:                gameID,
		Samples:               samples,
		RuntimeId:             "runtime:example",
		RuntimeRecipeRevision: "recipe-2",
		Source:                "overlay-frame-counter",
		ToolName:              "Example overlay",
		ToolVersion:           "2.0",
		SampleIntervalMs:      100,
		CpuScope:              proto.PerformanceCpuScope_PERFORMANCE_CPU_SCOPE_PROCESS,
		CpuNormalization:      proto.PerformanceCpuNormalization_PERFORMANCE_CPU_NORMALIZATION_WHOLE_MACHINE_PERCENT,
		MemoryKind:            proto.PerformanceMemoryKind_PERFORMANCE_MEMORY_KIND_WORKING_SET_BYTES,
		MemorySource:          "example-overlay",
		FpsUnit:               "frames/second",
		CpuUnit:               "percent",
		MemoryUnit:            "bytes",
	}
}

// PublishPerformance sends a batch through ResourceHost. `game.performance.publish` is required;
// the host binds the selected game and installation and rejects a foreign game.
func PublishPerformance(ctx context.Context, client proto.ResourceHostClient, request *proto.PerformanceSamplesRequest) (*proto.ReportMutationReply, error) {
	return client.GamePerformancePublishSamples(ctx, request)
}

// ListPerformance reads retained series for one game scope. Pass a selector to include other
// plugins' granted history; without one the answer is private-only.
func ListPerformance(ctx context.Context, client proto.GamePerformanceClient, scope *proto.ServiceContext, gameID string, selector *proto.CommonDataSelector, cursor string, pageSize uint32) (*proto.PerformanceListReply, error) {
	return client.List(ctx, &proto.PerformanceListRequest{
		Page: &proto.PageRequest{
			Context:              scope,
			Cursor:               cursor,
			PageSize:             pageSize,
			CommonDataCategories: selector,
		},
		GameId:            gameID,
		ComputerContextId: LocalComputerContext,
		ProjectionId:      PerformanceProjection,
	})
}

// QueryPerformance reads actual retained samples in a window; resolution selects real samples and
// never interpolates.
func QueryPerformance(ctx context.Context, client proto.GamePerformanceClient, scope *proto.ServiceContext, seriesID string, fromTime, toTime int64, resolutionMs uint32, selector *proto.CommonDataSelector) (*proto.PerformanceQuerySeriesReply, error) {
	return client.QuerySeries(ctx, &proto.PerformanceQuerySeriesRequest{
		Context:              scope,
		SeriesId:             seriesID,
		FromTime:             fromTime,
		ToTime:               toTime,
		ResolutionMs:         resolutionMs,
		Page:                 &proto.PageRequest{Context: scope, PageSize: 100},
		CommonDataCategories: selector,
	})
}

// AggregatePerformance aggregates one metric (fps, cpu_percent or memory_bytes) with average, min,
// max or nearest-rank p0..p100. A zero `toTime` leaves the upper bound open.
func AggregatePerformance(ctx context.Context, client proto.GamePerformanceClient, scope *proto.ServiceContext, seriesID, metric, aggregation string, selector *proto.CommonDataSelector) (*proto.PerformanceAggregateReply, error) {
	return client.Aggregate(ctx, &proto.PerformanceAggregateRequest{
		Context:              scope,
		SeriesId:             seriesID,
		FromTime:             0,
		ToTime:               0,
		Metric:               metric,
		Aggregation:          aggregation,
		CommonDataCategories: selector,
	})
}

// EndPerformance ends a series and cleans up its retained history. A later batch is refused with
// FailedPrecondition, so a stopped collector cannot reopen it.
func EndPerformance(ctx context.Context, client proto.ResourceHostClient, scope *proto.ServiceContext, key, seriesID string, expectedRevision, lastSequence uint64, endedAt int64) (*proto.MutationReply, error) {
	return client.GamePerformanceEndSeries(ctx, &proto.PerformanceEndSeriesRequest{
		Mutation: &proto.MutationContext{
			Context:          scope,
			IdempotencyKey:   key,
			ExpectedRevision: expectedRevision,
		},
		SeriesId:             seriesID,
		EndedAt:              endedAt,
		ExpectedLastSequence: lastSequence,
	})
}

// InspectLocation describes an approved folder by its opaque handle. The reply carries an opaque
// volume token and no path.
func InspectLocation(ctx context.Context, client proto.LocationsClient, scope *proto.ServiceContext, handle string) (*proto.LocationInspectReply, error) {
	return client.Inspect(ctx, &proto.LocationInspectRequest{
		Context:                scope,
		ApprovedResourceHandle: handle,
		RequestedDimensions:    append([]string(nil), LocationDimensions...),
		MaxEntries:             100,
	})
}

// MeasureLocation measures the approved scope from metadata alone. A missing or unreadable folder
// is reported through Availability, never as zero bytes.
func MeasureLocation(ctx context.Context, client proto.LocationsClient, scope *proto.ServiceContext, handle string) (*proto.LocationSizeReply, error) {
	return client.GetSize(ctx, &proto.LocationSizeRequest{
		Context:                scope,
		ApprovedResourceHandle: handle,
	})
}

// ListSaveProfiles lists the caller's own profiles. A common-category selector is answered with
// Unimplemented: save profiles stay owner-only.
func ListSaveProfiles(ctx context.Context, client proto.SavesClient, scope *proto.ServiceContext, installationID string, selector *proto.CommonDataSelector) (*proto.SaveProfileListReply, error) {
	return client.ListProfiles(ctx, &proto.SaveProfileListRequest{
		Context:        scope,
		InstallationId: installationID,
		PageSize:       20,
		SharedRecords:  selector,
	})
}

// ReadSaveProfile reads one of the caller's own profiles. The approved folder appears as a
// session-bound handle; when the approval is gone the handle is empty.
func ReadSaveProfile(ctx context.Context, client proto.SavesClient, scope *proto.ServiceContext, gameID, profileID string) (*proto.SaveProfileReply, error) {
	return client.GetProfile(ctx, &proto.SaveProfileGetRequest{
		Context:   scope,
		GameId:    gameID,
		ProfileId: profileID,
	})
}

// ListSharedRuntimes lists runtime records with the explicit runtime-catalog selector. The shared
// projection carries producer provenance and validated extension namespaces but an empty
// executable handle; a missing current grant is PermissionDenied.
func ListSharedRuntimes(ctx context.Context, client proto.ResourceHostClient, scope *proto.ServiceContext) (*proto.RuntimesListReply, error) {
	return client.RuntimesList(ctx, &proto.DomainListRequest{
		Page: &proto.PageRequest{
			Context:              scope,
			PageSize:             50,
			CommonDataCategories: CategorySelector(proto.CommonDataCategory_COMMON_DATA_CATEGORY_RUNTIME_CATALOG),
		},
	})
}

// ContentAvailability reads one installation's safe content/runtime identities. Several contents
// may share one runtime; each row names content and runtime identities, never a path or handle.
func ContentAvailability(ctx context.Context, client proto.ResourceHostClient, scope *proto.ServiceContext, installationID string) (*proto.LibraryGetReply, error) {
	return client.LibraryGet(ctx, &proto.LibraryGetRequest{
		Context:              scope,
		InstallationId:       installationID,
		CommonDataCategories: CategorySelector(proto.CommonDataCategory_COMMON_DATA_CATEGORY_CONTENT_AVAILABILITY),
	})
}

// CustomDatasetBatch builds a versioned custom Struct publication. Core interprets the dataset and
// schema identity, not the field meaning; a consumer receives only the fields its review grants.
func CustomDatasetBatch(scope *proto.ServiceContext, key, datasetID, schemaID string, schemaVersion uint32, value *structpb.Struct, sequence uint64, observationID string) *proto.DataPublishRequest {
	return &proto.DataPublishRequest{
		Mutation: &proto.MutationContext{
			Context:        scope,
			IdempotencyKey: key,
		},
		DatasetId:           datasetID,
		SchemaId:            schemaID,
		SchemaVersion:       schemaVersion,
		Value:               value,
		Durability:          proto.StorageDurability_STORAGE_DURABILITY_SESSION_ONLY,
		TtlSeconds:          86400,
		ResourceId:          "game-notes-producer",
		ObservedAt:          1760000000000,
		Sequence:            sequence,
		SourceObservationId: observationID,
	}
}

// OpaqueDatasetBatch builds an opaque custom-envelope publication. A consumer needs an explicit
// `opaque_payload` field grant and receives the whole payload.
func OpaqueDatasetBatch(scope *proto.ServiceContext, key, datasetID, schemaID string, payload []byte, sequence uint64, observationID string) *proto.DataPublishRequest {
	return &proto.DataPublishRequest{
		Mutation: &proto.MutationContext{
			Context:        scope,
			IdempotencyKey: key,
		},
		DatasetId:           datasetID,
		SchemaId:            schemaID,
		SchemaVersion:       1,
		OpaquePayload:       append([]byte(nil), payload...),
		Durability:          proto.StorageDurability_STORAGE_DURABILITY_SESSION_ONLY,
		TtlSeconds:          86400,
		ResourceId:          "game-notes-binary-producer",
		ObservedAt:          1760000000000,
		Sequence:            sequence,
		SourceObservationId: observationID,
	}
}

// PublishCustom sends a custom Struct or opaque publication; `data.publish` is required.
func PublishCustom(ctx context.Context, client proto.ResourceHostClient, request *proto.DataPublishRequest) (*proto.DataMutationReply, error) {
	return client.DataPublish(ctx, request)
}

// ListCustom reads one custom dataset under the caller's current custom-field policy. Without a
// matching source/recipient policy the answer is empty, never a narrowed or partial record.
func ListCustom(ctx context.Context, client proto.ResourceHostClient, scope *proto.ServiceContext, datasetID string) (*proto.DataListReply, error) {
	return client.DataList(ctx, &proto.DataListRequest{
		Page:      &proto.PageRequest{Context: scope, PageSize: 10},
		DatasetId: datasetID,
	})
}
