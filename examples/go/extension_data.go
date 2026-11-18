// Hybrid typed records and versioned custom-data wire shapes.
//
// This package has protobuf bindings only, not a GameNode SDK or a gRPC client. The functions
// build and manipulate local messages; they do not claim the corresponding host RPCs are served.
package main

import (
	"crypto/sha256"

	"example.com/gamenode/plugin-example/proto"
	goproto "google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/structpb"
)

const (
	knownRuntimeNamespace  = "org.example.runtime"
	futureRuntimeNamespace = "org.vendor.future-runtime"
	runtimeV1Schema        = `{"type":"object","properties":{"display_family":{"type":"string"}},"additionalProperties":false}`
	futureRuntimeV9Schema  = `{"type":"object","properties":{"new_hint":{"type":"string"}},"additionalProperties":true}`
)

func extension(schemaID string, version uint32, schemaJSON string, value map[string]any) (*proto.ExtensionPayload, error) {
	data, err := structpb.NewStruct(value)
	if err != nil {
		return nil, err
	}
	digest := sha256.Sum256([]byte(schemaJSON))
	return &proto.ExtensionPayload{
		SchemaId: schemaID, SchemaVersion: version, SchemaDigest: digest[:], Data: data,
	}, nil
}

// HybridRuntime keeps authority-bearing identity, revision, version and executable handle in
// typed fields. The future namespace is carried opaquely by clients that do not know its schema.
func HybridRuntime() (*proto.RuntimeRecord, error) {
	known, err := extension("org.example.runtime.v1", 1, runtimeV1Schema, map[string]any{"display_family": "desktop"})
	if err != nil {
		return nil, err
	}
	future, err := extension("org.vendor.future-runtime.v9", 9, futureRuntimeV9Schema, map[string]any{"new_hint": "preserve without parsing"})
	if err != nil {
		return nil, err
	}
	return &proto.RuntimeRecord{
		Id: "runtime:example", Name: "Example Runtime", Version: "2.4.1",
		ExecutableHandle: "opaque-runtime-handle", SupportedFormats: []string{"rom.example"},
		Extensions: map[string]*proto.ExtensionPayload{
			knownRuntimeNamespace:  known,
			futureRuntimeNamespace: future,
		},
		Revision: 7, ObservedAt: 1800000000,
	}, nil
}

// ApplyKnownNamespacePatch locally updates one typed field and one known extension while keeping
// every unselected namespace intact. The returned ExtensionPatch is a wire shape only: the host
// runtime update operation is not currently served, and the host—not the client—assigns revision.
func ApplyKnownNamespacePatch(current *proto.RuntimeRecord) (*proto.RuntimeRecord, *proto.ExtensionPatch, error) {
	known, err := extension("org.example.runtime.v1", 1, runtimeV1Schema, map[string]any{"display_family": "handheld"})
	if err != nil {
		return nil, nil, err
	}
	patch := &proto.ExtensionPatch{
		ExpectedRevision: current.GetRevision(),
		TypedFieldMask:   []string{"name"},
		Namespaces: []*proto.ExtensionNamespacePatch{{
			Namespace: knownRuntimeNamespace,
			Operation: &proto.ExtensionNamespacePatch_Set{Set: known},
		}},
	}
	updated := goproto.Clone(current).(*proto.RuntimeRecord)
	updated.Name = "Example Runtime (handheld)"
	if updated.Extensions == nil {
		updated.Extensions = make(map[string]*proto.ExtensionPayload)
	}
	updated.Extensions[knownRuntimeNamespace] = goproto.Clone(known).(*proto.ExtensionPayload)
	return updated, patch, nil
}

// ProjectStructKeys demonstrates a consumer-side view over approved fields. It is not an
// authorization operation; a real consumer must let the host enforce field grants and projection.
func ProjectStructKeys(input *structpb.Struct, allowed ...string) *structpb.Struct {
	projected := &structpb.Struct{Fields: make(map[string]*structpb.Value, len(allowed))}
	for _, key := range allowed {
		if value, ok := input.GetFields()[key]; ok {
			projected.Fields[key] = goproto.Clone(value).(*structpb.Value)
		}
	}
	return projected
}

// CustomDatasetWireShape constructs the versioned Struct envelope. DataPublish is served by the
// desktop host; this protobuf-only fixture does not send the request to it.
func CustomDatasetWireShape() (*proto.DataPublishRequest, error) {
	value, err := structpb.NewStruct(map[string]any{
		"game_key": "game:example", "note": "remember controller layout",
	})
	if err != nil {
		return nil, err
	}
	return &proto.DataPublishRequest{
		DatasetId: "org.example.game-notes", SchemaId: "org.example.game-notes.v2",
		SchemaVersion: 2, Value: value,
		Durability: proto.StorageDurability_STORAGE_DURABILITY_SESSION_ONLY,
		TtlSeconds: 86400, ResourceId: "game-notes-producer",
		ObservedAt: 1800000000, Sequence: 1,
		SourceObservationId: "game-notes-observation-1",
	}, nil
}

// OpaqueDatasetWireShape builds an opaque custom-dataset publication. Consumers need an explicit
// opaque_payload field grant; this fixture has no RPC client and only returns the wire message.
func OpaqueDatasetWireShape() *proto.DataPublishRequest {
	return &proto.DataPublishRequest{
		DatasetId: "org.example.game-notes.binary", SchemaId: "org.example.game-notes.binary.v1",
		SchemaVersion: 1, OpaquePayload: []byte{0, 1, 2, 255},
		Durability: proto.StorageDurability_STORAGE_DURABILITY_SESSION_ONLY,
		TtlSeconds: 86400, ResourceId: "game-notes-producer",
		ObservedAt: 1800000000, Sequence: 1,
		SourceObservationId: "game-notes-binary-observation-1",
	}
}

// OpaqueBytesWireShape demonstrates private Storage bytes, which remain separate from shared data.
func OpaqueBytesWireShape(key string, value []byte) *proto.StoragePutRequest {
	return &proto.StoragePutRequest{
		Key: key, Value: append([]byte(nil), value...),
		Durability: proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT,
	}
}

func main() {}
