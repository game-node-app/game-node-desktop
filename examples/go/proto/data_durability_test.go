package proto_test

import (
	"testing"

	"example.com/gamenode/plugin-example/proto"
	goproto "google.golang.org/protobuf/proto"
)

// The durability field is additive: a reply or record without it keeps every other field and
// reads as unspecified, so an older host or client is never mistaken for a durable record.
func TestDataDurabilityIsAdditive(t *testing.T) {
	raw, err := goproto.Marshal(&proto.DataMutationReply{RecordId: "record-1", Revision: 3})
	if err != nil {
		t.Fatal(err)
	}
	var absent proto.DataMutationReply
	if err := goproto.Unmarshal(raw, &absent); err != nil {
		t.Fatal(err)
	}
	if absent.GetRecordId() != "record-1" || absent.GetRevision() != 3 {
		t.Fatalf("additive field changed existing values: %+v", &absent)
	}
	if got := absent.GetDurability(); got != proto.StorageDurability_STORAGE_DURABILITY_UNSPECIFIED {
		t.Fatalf("absent durability = %v, want unspecified", got)
	}

	durable := &proto.DataRecord{
		RecordId:   "record-2",
		Revision:   4,
		Durability: proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT,
	}
	raw, err = goproto.Marshal(durable)
	if err != nil {
		t.Fatal(err)
	}
	var restored proto.DataRecord
	if err := goproto.Unmarshal(raw, &restored); err != nil {
		t.Fatal(err)
	}
	if got := restored.GetDurability(); got != proto.StorageDurability_STORAGE_DURABILITY_PERSISTENT {
		t.Fatalf("durability = %v, want persistent", got)
	}
	if restored.GetRecordId() != durable.GetRecordId() || restored.GetRevision() != durable.GetRevision() {
		t.Fatalf("round trip changed record: %+v", &restored)
	}
}
