package proto

import (
	"testing"

	"google.golang.org/protobuf/proto"
)

// These are synthetic wire fixtures: handles are opaque host-issued placeholders, not paths.
// Proposals are inputs to the host review RPC; execution carries only its approved recipe reference.
func TestLaunchConsentFixturesKeepReviewAndInstallSeparate(t *testing.T) {
	fixtures := []struct {
		name    string
		request *CustomLaunchRequest
	}{
		{
			name: "Proton-style runtime wrapper",
			request: &CustomLaunchRequest{GameId: "fixture-game", InstallationId: "fixture-install",
				ExpectedRevision: 12, RuntimeHandle: "runtime-handle", ContentId: "content-variant-1",
				Argv: []string{"--literal value"}, WorkingDirectoryHandle: "cwd-handle",
				EnvironmentSet: map[string]string{"A": "value with spaces"}, EnvironmentUnset: []string{"B"}},
		},
		{
			name: "known mod layer",
			request: &CustomLaunchRequest{GameId: "fixture-game", InstallationId: "fixture-install",
				ExpectedRevision: 7, ExecutableHandle: "opaque-executable-handle",
				RuntimeHandle: "opaque-mod-layer-handle", Argv: []string{"--profile", "safe fixture"}},
		},
		{
			name: "environment only",
			request: &CustomLaunchRequest{GameId: "fixture-game", InstallationId: "fixture-install",
				ExpectedRevision: 7, EnvironmentSet: map[string]string{"FIXTURE_MODE": "enabled"},
				EnvironmentUnset: []string{"FIXTURE_LEGACY_MODE"}},
		},
	}
	for _, fixture := range fixtures {
		t.Run(fixture.name, func(t *testing.T) {
			fixture.request.UserIntentId = "one-use-host-intent"
			fixture.request.IdempotencyKey = "fixture-idempotency-key"
			wire, err := proto.Marshal(fixture.request)
			if err != nil {
				t.Fatal(err)
			}
			proposal := new(CustomLaunchRequest)
			if err := proto.Unmarshal(wire, proposal); err != nil {
				t.Fatal(err)
			}
			if !proto.Equal(proposal, fixture.request) {
				t.Fatal("proposal fields changed on the wire")
			}
			if proposal.UserIntentId != "one-use-host-intent" {
				t.Fatal("proposal must carry the host user intent")
			}

			// No executable, argv, environment, runtime, or URI can be supplied when
			// requesting execution: the host resolves the exact reviewed revision.
			execution := &CustomLaunchRequest{
				GameId: proposal.GameId, InstallationId: proposal.InstallationId,
				ExpectedRevision: proposal.ExpectedRevision, UserIntentId: "one-use-host-intent",
				IdempotencyKey: "execution-idempotency-key", RecipeId: "host-approved-recipe", RecipeRevision: 3,
			}
			if execution.ExecutableHandle != "" || len(execution.Argv) != 0 || execution.WorkingDirectoryHandle != "" ||
				len(execution.EnvironmentSet) != 0 || len(execution.EnvironmentUnset) != 0 ||
				execution.RuntimeHandle != "" || execution.Uri != "" || execution.ContentId != "" {
				t.Fatal("execution fixture must not replace reviewed launch fields")
			}
		})
	}

	// Launch approval is not an installation grant or installation intent.
	launchIntent := "one-use-host-intent"
	install := &GameInstallRequest{
		GameId: "fixture-game", InstallationId: "fixture-install",
		UserIntentId: "separate-install-intent", IdempotencyKey: "install-idempotency-key",
	}
	if install.UserIntentId == launchIntent {
		t.Fatal("launch consent must not authorize installation")
	}
}

func TestLaunchConsentUsesCustomReviewRPCAndSeparateInstallRPC(t *testing.T) {
	resourceHost := File_plugin_proto.Services().ByName("ResourceHost")
	if resourceHost == nil {
		t.Fatal("ResourceHost service is missing")
	}
	methods := resourceHost.Methods()
	proposal := methods.ByName("GamesProposeCustomLaunch")
	execution := methods.ByName("GamesRequestCustomLaunch")
	install := methods.ByName("GamesRequestInstall")
	fileWrite := methods.ByName("FilesWrite")
	if proposal == nil || execution == nil || install == nil || fileWrite == nil {
		t.Fatal("custom-launch review, install, and file-write RPCs must be declared")
	}
	if proposal.Input().Name() != "CustomLaunchRequest" || execution.Input().Name() != "CustomLaunchRequest" {
		t.Fatal("custom launches must go through the host custom-launch review request")
	}
	if install.Input().Name() != "GameInstallRequest" {
		t.Fatal("installation must use its separate host request")
	}
	if fileWrite.Input().Name() != "FilesWriteRequest" {
		t.Fatal("file writes must use their separate host request and files.write grant")
	}
}
