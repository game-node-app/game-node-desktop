package proto

import (
	"testing"

	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/structpb"
)

func TestWireRoundTripPreservesUnknownNamespaceAndExactRevisions(t *testing.T) {
	selected, err := structpb.NewStruct(map[string]any{"note": "before"})
	if err != nil { t.Fatal(err) }
	unknown, err := structpb.NewStruct(map[string]any{"vendor": "keep"})
	if err != nil { t.Fatal(err) }
	record := &RuntimeRecord{
		Id: "runtime-1", Revision: ^uint64(0), ObservedAt: int64(^uint64(0) >> 1),
		Extensions: map[string]*ExtensionPayload{
			"org.example.runtime": {SchemaId: "org.example.runtime.v1", SchemaVersion: 1, SchemaDigest: make([]byte, 32), Data: selected},
			"org.other.unknown": {SchemaId: "org.other.unknown.v9", SchemaVersion: 9, SchemaDigest: make([]byte, 32), Data: unknown},
		},
	}
	wire, err := proto.Marshal(record)
	if err != nil { t.Fatal(err) }
	decoded := new(RuntimeRecord)
	if err := proto.Unmarshal(wire, decoded); err != nil { t.Fatal(err) }
	if decoded.Revision != ^uint64(0) || decoded.ObservedAt != int64(^uint64(0)>>1) {
		t.Fatalf("precise typed fields changed: revision=%d observed_at=%d", decoded.Revision, decoded.ObservedAt)
	}
	// An older consumer patches only its selected namespace. The unknown map
	// entry is carried through without interpreting or reconstructing it.
	updated, err := structpb.NewStruct(map[string]any{"note": "after"})
	if err != nil { t.Fatal(err) }
	decoded.Extensions["org.example.runtime"].Data = updated
	patchedWire, err := proto.Marshal(decoded)
	if err != nil { t.Fatal(err) }
	patched := new(RuntimeRecord)
	if err := proto.Unmarshal(patchedWire, patched); err != nil { t.Fatal(err) }
	if !proto.Equal(patched.Extensions["org.other.unknown"], record.Extensions["org.other.unknown"]) {
		t.Fatal("unselected extension namespace was not preserved")
	}
}

func TestDomainContractsAndSelectedRecordSelectorRoundTrip(t *testing.T) {
	profile := &SaveProfile{
		ProfileId: "save-profile-1",
		GameId: "game-1",
		Revision: ^uint64(0),
		Directories: []*SaveDirectory{{ApprovedResourceHandle: "opaque-handle", NamingPatterns: []string{"*.sav"}}},
	}
	wire, err := proto.Marshal(profile)
	if err != nil { t.Fatal(err) }
	decoded := new(SaveProfile)
	if err := proto.Unmarshal(wire, decoded); err != nil { t.Fatal(err) }
	if decoded.Revision != ^uint64(0) || decoded.Directories[0].ApprovedResourceHandle != "opaque-handle" {
		t.Fatalf("typed save-profile contract changed: %#v", decoded)
	}

	selector := &CommonDataSelector{
		Categories: []CommonDataCategory{CommonDataCategory_COMMON_DATA_CATEGORY_RUNTIME_CATALOG},
		SelectedRecordIds: []string{"runtime:one", "runtime:two"},
	}
	wire, err = proto.Marshal(selector)
	if err != nil { t.Fatal(err) }
	decodedSelector := new(CommonDataSelector)
	if err := proto.Unmarshal(wire, decodedSelector); err != nil { t.Fatal(err) }
	if len(decodedSelector.Categories) != 1 || len(decodedSelector.SelectedRecordIds) != 2 {
		t.Fatalf("category and selected-record selectors were not preserved: %#v", decodedSelector)
	}
}
