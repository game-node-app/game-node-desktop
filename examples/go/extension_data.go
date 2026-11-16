// Hybrid typed records and versioned custom-data wire shapes.
//
// This package has protobuf bindings only, not a GameNode SDK or a gRPC client. The functions
// build and manipulate local messages; they do not claim the corresponding host RPCs are served.
package main

import (
	"crypto/sha256"

	"example.com/gamenode/plugin-example/proto"
	"google.golang.org/protobuf/proto"
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
	updated := proto.Clone(current).(*proto.RuntimeRecord)
	updated.Name = "Example Runtime (handheld)"
	if updated.Extensions == nil {
		updated.Extensions = make(map[string]*proto.ExtensionPayload)
	}
	updated.Extensions[knownRuntimeNamespace] = proto.Clone(known).(*proto.ExtensionPayload)
	return updated, patch, nil
}

// ProjectStructKeys demonstrates a consumer-side view over approved fields. It is not an
// authorization operation; a real consumer must let the host enforce field grants and projection.
func ProjectStructKeys(input *structpb.Struct, allowed ...string) *structpb.Struct {
	projected := &structpb.Struct{Fields: make(map[string]*structpb.Value, len(allowed))}
	for _, key := range allowed {
		if value, ok := input.GetFields()[key]; ok {
			projected.Fields[key] = proto.Clone(value).(*structpb.Value)
		}
	}
	return projected
}

// CustomDatasetWireShape constructs the versioned Struct envelope. ResourceHost.DataPublish is
// currently unserved by the desktop host, so this message is not publishable through this fixture.
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
		TtlSeconds: 86400,
	}, nil
}

// OpaqueBytesWireShape demonstrates the existing private Storage byte field. It is not a public
// or cross-plugin dataset; this Go fixture has no RPC client, so it returns only the wire message.
func OpaqueBytesWireShape(key string, value []byte) *proto.StoragePutRequest {
	return &proto.StoragePutRequest{
		Key: key, Value: append([]byte(nil), value...),
		Durability: proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT,
	}
}

func main() {}
