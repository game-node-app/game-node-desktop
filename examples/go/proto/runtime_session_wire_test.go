package proto

import (
	"testing"

	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/reflect/protoreflect"
)

func TestRuntimeSessionReportAndCorrelatedResultRoundTrip(t *testing.T) {
	report := &RuntimeSessionReport{
		CorrelationId:       "correlation-1",
		GameId:              "42",
		InstallationId:      "steam:42",
		ContentId:           "rom:variant-a",
		RuntimeId:           "emulator:one",
		RuntimeInstanceId:   "instance-a",
		ReporterSessionId:   "host-session-a",
		Sequence:            7,
		ObservedAt:          1_800_000_000,
		Transition:          RuntimeSessionTransition_RUNTIME_SESSION_TRANSITION_CONTENT_CHANGED,
		SourceObservationId: "status-observation-7",
	}
	wire, err := proto.Marshal(report)
	if err != nil {
		t.Fatal(err)
	}
	decodedReport := new(RuntimeSessionReport)
	if err := proto.Unmarshal(wire, decodedReport); err != nil {
		t.Fatal(err)
	}
	if !proto.Equal(report, decodedReport) {
		t.Fatalf("runtime-session fields changed: got %#v, want %#v", decodedReport, report)
	}

	result := &RuntimeSessionResult{
		CorrelationId:    report.CorrelationId,
		Outcome:          RuntimeSessionOutcome_RUNTIME_SESSION_OUTCOME_ACCEPTED,
		HostSessionId:    "host-measured-session-1",
		AcceptedSequence: report.Sequence,
		AcceptedAt:       report.ObservedAt,
	}
	wire, err = proto.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}
	decodedResult := new(RuntimeSessionResult)
	if err := proto.Unmarshal(wire, decodedResult); err != nil {
		t.Fatal(err)
	}
	if !proto.Equal(result, decodedResult) || decodedResult.CorrelationId != decodedReport.CorrelationId {
		t.Fatalf("correlated runtime-session result changed: %#v", decodedResult)
	}
}

func TestRuntimeSessionWireDoesNotCarryProcessOrCredentialHandles(t *testing.T) {
	fields := (&RuntimeSessionReport{}).ProtoReflect().Descriptor().Fields()
	for _, forbidden := range []string{"pid", "process_handle", "file_handle", "session_credential", "launch_token"} {
		if fields.ByName(protoreflect.Name(forbidden)) != nil {
			t.Fatalf("runtime-session report must not contain %q", forbidden)
		}
	}
}
