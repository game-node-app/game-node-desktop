package proto

import (
	"testing"

	"google.golang.org/protobuf/proto"
)

func TestRuntimeArgumentTemplateAndContentIdentityRoundTrip(t *testing.T) {
	runtime := &RuntimeRecord{
		Id: "runtime-1",
		Platform: "linux",
		ArgumentTemplate: []*RuntimeArgumentTemplate{
			{Value: &RuntimeArgumentTemplate_Literal{Literal: "--game"}},
			{Value: &RuntimeArgumentTemplate_ContentPath{ContentPath: &ContentPathArgument{}}},
		},
	}
	wire, err := proto.Marshal(runtime)
	if err != nil {
		t.Fatal(err)
	}
	decoded := new(RuntimeRecord)
	if err := proto.Unmarshal(wire, decoded); err != nil {
		t.Fatal(err)
	}
	if decoded.Platform != "linux" || len(decoded.ArgumentTemplate) != 2 {
		t.Fatalf("typed runtime fields changed: %#v", decoded)
	}
	if decoded.ArgumentTemplate[0].GetLiteral() != "--game" || decoded.ArgumentTemplate[1].GetContentPath() == nil {
		t.Fatalf("typed argv tokens changed: %#v", decoded.ArgumentTemplate)
	}

	content := &ContentRegistrationRequest{ContentId: "content-variant-1", ContentHandle: "session-handle"}
	wire, err = proto.Marshal(content)
	if err != nil {
		t.Fatal(err)
	}
	decodedContent := new(ContentRegistrationRequest)
	if err := proto.Unmarshal(wire, decodedContent); err != nil {
		t.Fatal(err)
	}
	if decodedContent.ContentId != "content-variant-1" || decodedContent.ContentHandle != "session-handle" {
		t.Fatalf("stable content identity was not preserved: %#v", decodedContent)
	}
}
