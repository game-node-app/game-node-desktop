package provider

import (
	"time"

	"example.com/gamenode/plugins/go-grpc/proto"
	pbproto "google.golang.org/protobuf/proto"
)

// Implementation is the small provider contract consumed by Client.Run.
type Implementation interface {
	Handle(*proto.ProviderCommand) []*proto.ProviderEvent
}

// Demo returns fictional discovery, catalog and playtime data for the runnable example.
type Demo struct{}

func (Demo) Handle(command *proto.ProviderCommand) []*proto.ProviderEvent {
	ctx := pbproto.Clone(command.GetContext()).(*proto.ServiceContext)
	now := time.Now().Unix()
	result := &proto.OperationResult{
		Disposition: proto.OperationDisposition_OPERATION_DISPOSITION_COMPLETED,
		ObservedAt:  now,
	}
	var events []*proto.ProviderEvent
	switch command.GetOperation() {
	case proto.ProviderOperation_PROVIDER_OPERATION_DISCOVER:
		result.Discovery = &proto.DiscoveryResult{
			ProviderId: ProviderID,
			Records: []*proto.DiscoveryRecord{{
				Id:         "demo-installed-game",
				Name:       "Example Adventure",
				InstallDir: "/games/example-adventure",
				Fields:     map[string]string{"source": "go-grpc-example"},
			}},
		}
	case proto.ProviderOperation_PROVIDER_OPERATION_SYNC_CATALOG:
		events = append(events, &proto.ProviderEvent{
			Context: ctx,
			Event: &proto.ProviderEvent_CatalogChunk{CatalogChunk: &proto.CatalogChunk{
				Category: proto.CatalogCategory_CATALOG_CATEGORY_OWNED,
				Index:    0,
				Games: []*proto.CatalogGame{{
					Id: "demo-catalog-game", Name: "Example Adventure",
					Fields: map[string]string{"edition": "standard"},
				}},
			}},
		})
		result.Catalogs = []*proto.CatalogCompletion{{
			Category: proto.CatalogCategory_CATALOG_CATEGORY_OWNED,
			Outcome:  &proto.CatalogCompletion_CompletedRecords{CompletedRecords: 1},
		}}
	case proto.ProviderOperation_PROVIDER_OPERATION_READ_COUNTER:
		lastPlayed := now - 3600
		result.Counters = []*proto.CounterObservation{{
			Id: "demo-catalog-game", Seconds: 7200, LastPlayed: &lastPlayed,
		}}
	default:
		result.Disposition = proto.OperationDisposition_OPERATION_DISPOSITION_FAILED
		result.Failure = &proto.PluginFailure{Code: "unsupported_operation", Message: "operation is outside this example"}
	}
	events = append(events, &proto.ProviderEvent{
		Context: ctx,
		Event:   &proto.ProviderEvent_Result{Result: result},
	})
	return events
}
