package main

import (
	"errors"
	"testing"

	"example.com/gamenode/plugin-example/proto"
	pbproto "google.golang.org/protobuf/proto"
)

// These cases mirror the Rust extension recipes against the checked-in shared fixture, so both
// clients construct the same hybrid record, custom envelope and opaque envelope. The Rust example
// also runs the same values against the desktop fixture host in its service tests.

func TestHybridExtensionIdentityAndProjectionMatchTheSharedContract(t *testing.T) {
	contract := exchangeContract(t)
	extension := object(t, contract["extension"])

	record, err := HybridRuntime()
	if err != nil {
		t.Fatal(err)
	}
	known := record.GetExtensions()[text(t, extension["known_namespace"])]
	if known.GetSchemaId() != text(t, extension["known_schema_id"]) ||
		known.GetSchemaVersion() != uint32(number(t, extension["known_schema_version"])) {
		t.Fatalf("known extension identity = %s v%d", known.GetSchemaId(), known.GetSchemaVersion())
	}
	future := record.GetExtensions()[text(t, extension["future_namespace"])]
	if future.GetSchemaId() != text(t, extension["future_schema_id"]) ||
		future.GetSchemaVersion() != uint32(number(t, extension["future_schema_version"])) {
		t.Fatalf("future extension identity = %s v%d", future.GetSchemaId(), future.GetSchemaVersion())
	}
	if got := known.GetData().GetFields()[text(t, extension["known_field"])].GetStringValue(); got != text(t, extension["known_value"]) {
		t.Fatalf("known field = %q", got)
	}
	if got := future.GetData().GetFields()[text(t, extension["future_field"])].GetStringValue(); got != text(t, extension["future_value"]) {
		t.Fatalf("future field = %q", got)
	}

	// Schema identity is part of the contract: the client knows v1 and refuses to interpret v9.
	if err := ValidateKnownSchema(known); err != nil {
		t.Fatalf("known schema rejected: %v", err)
	}
	if err := ValidateKnownSchema(future); !errors.Is(err, ErrUnsupportedSchema) {
		t.Fatalf("future schema error = %v, want unsupported", err)
	}

	// An older client patches only its namespace; the future namespace survives unchanged.
	updated, patch, err := ApplyKnownNamespacePatch(record)
	if err != nil {
		t.Fatal(err)
	}
	if patch.GetExpectedRevision() != record.GetRevision() {
		t.Fatalf("patch revision = %d, want %d", patch.GetExpectedRevision(), record.GetRevision())
	}
	if !pbproto.Equal(updated.GetExtensions()[text(t, extension["future_namespace"])], future) {
		t.Fatal("unselected future namespace was not preserved")
	}
	if got := updated.GetExtensions()[text(t, extension["known_namespace"])].GetData().GetFields()[text(t, extension["known_field"])].GetStringValue(); got != "handheld" {
		t.Fatalf("patched known field = %q", got)
	}

	// Projection keeps only a declared, granted field; it grants no authority itself.
	projected := ProjectStructKeys(known.GetData(), text(t, extension["known_field"]))
	if len(projected.GetFields()) != 1 || projected.GetFields()[text(t, extension["known_field"])] == nil {
		t.Fatalf("projected fields = %v", projected.GetFields())
	}
}

func TestCustomAndOpaqueEnvelopesMatchTheSharedContract(t *testing.T) {
	contract := exchangeContract(t)
	extension := object(t, contract["extension"])
	customFields := object(t, extension["custom_fields"])

	custom, err := CustomDatasetWireShape()
	if err != nil {
		t.Fatal(err)
	}
	if custom.GetDatasetId() != text(t, extension["custom_dataset_id"]) ||
		custom.GetSchemaId() != text(t, extension["custom_schema_id"]) ||
		custom.GetSchemaVersion() != uint32(number(t, extension["custom_schema_version"])) {
		t.Fatalf("custom identity = %s / %s v%d", custom.GetDatasetId(), custom.GetSchemaId(), custom.GetSchemaVersion())
	}
	for field, value := range customFields {
		want, ok := value.(string)
		if !ok {
			t.Fatalf("custom field %q is not text", field)
		}
		if got := custom.GetValue().GetFields()[field].GetStringValue(); got != want {
			t.Fatalf("custom field %q = %q, want %q", field, got, want)
		}
	}
	// A consumer sees only the granted field; the ungranted note is dropped.
	granted := text(t, list(t, extension["projection_paths"])[0])
	projected := ProjectStructKeys(custom.GetValue(), granted)
	if len(projected.GetFields()) != 1 || projected.GetFields()[granted] == nil {
		t.Fatalf("projected custom fields = %v", projected.GetFields())
	}

	// A later schema version is retained as a label for custom data, not reinterpreted.
	future, err := CustomDatasetWireShapeWithVersion(uint32(number(t, extension["unsupported_schema_version"])))
	if err != nil {
		t.Fatal(err)
	}
	if future.GetSchemaVersion() != uint32(number(t, extension["unsupported_schema_version"])) ||
		future.GetDatasetId() != custom.GetDatasetId() ||
		!pbproto.Equal(future.GetValue(), custom.GetValue()) {
		t.Fatalf("versioned custom envelope = %v", future)
	}

	// Opaque bytes have no Struct to project; the grant names the whole payload field.
	opaque := OpaqueDatasetWireShape()
	wantBytes := []byte{0, 1, 2, 255}
	if len(opaque.GetOpaquePayload()) != len(wantBytes) || opaque.GetValue() != nil {
		t.Fatalf("opaque envelope = %v", opaque)
	}
	for index, value := range wantBytes {
		if opaque.GetOpaquePayload()[index] != value {
			t.Fatalf("opaque byte %d = %d, want %d", index, opaque.GetOpaquePayload()[index], value)
		}
	}
	if text(t, extension["opaque_grant_field"]) != "opaque_payload" {
		t.Fatal("the opaque grant field changed")
	}

	// An opaque envelope is not a known typed namespace; interpreting it must fail explicitly
	// instead of returning an empty projection.
	if err := ValidateKnownSchema(&proto.ExtensionPayload{
		SchemaId:      opaque.GetSchemaId(),
		SchemaVersion: opaque.GetSchemaVersion(),
		SchemaDigest:  make([]byte, 32),
		Data:          custom.GetValue(),
	}); !errors.Is(err, ErrUnsupportedSchema) {
		t.Fatalf("opaque envelope error = %v, want unsupported", err)
	}
}
