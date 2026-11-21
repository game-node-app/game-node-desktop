//go:build !windows

package provider

import (
	"context"
	"fmt"
	"reflect"
	"sync"
	"testing"
	"time"

	"example.com/gamenode/plugins/go-grpc/proto"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	pbproto "google.golang.org/protobuf/proto"
)

// launchReviewHost is the host fixture for the common custom-launch contract. It stores the
// proposal, shows the reviewed plan when execution is requested (modeled by the ordered answers)
// and reports the job outcome. A request without the grant is denied before any review or
// dispatch, as in the host; nothing here replaces the approved launch fields.
type launchReviewHost struct {
	proto.UnimplementedResourceHostServer

	credential []byte
	granted    bool
	answers    []bool

	mu            sync.Mutex
	proposals     []*proto.CustomLaunchRequest
	requests      []*proto.CustomLaunchRequest
	reviewedPlans []*proto.CustomLaunchRequest
	jobs          map[string]*proto.JobReply
	launches      int
}

func newLaunchReviewHost(credential []byte, answers ...bool) *launchReviewHost {
	return &launchReviewHost{
		credential: credential,
		granted:    true,
		answers:    answers,
		jobs:       make(map[string]*proto.JobReply),
	}
}

func (h *launchReviewHost) authorize(ctx *proto.ServiceContext) error {
	if ctx.GetSessionId() != "fixture-session" ||
		!reflect.DeepEqual(ctx.GetSessionCredential(), h.credential) ||
		ctx.GetSessionKind() != proto.SessionKind_SESSION_KIND_EXTENSION ||
		ctx.GetTargetGame() == nil {
		return status.Error(codes.Unauthenticated, "fixture extension context mismatch")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.granted {
		return status.Error(codes.PermissionDenied, "game.custom_launch is not granted")
	}
	return nil
}

// GamesProposeCustomLaunch validates and stores a recipe. It never reviews, starts a process or
// answers with a job: execution is a separate, explicitly approved request.
func (h *launchReviewHost) GamesProposeCustomLaunch(_ context.Context, request *proto.CustomLaunchRequest) (*proto.CustomLaunchReply, error) {
	if err := h.authorize(request.GetContext()); err != nil {
		return nil, err
	}
	if request.GetGameId() == "" || request.GetInstallationId() == "" || request.GetExpectedRevision() == 0 ||
		request.GetIdempotencyKey() == "" || request.GetRecipeId() != "" || request.GetRecipeRevision() != 0 {
		return nil, status.Error(codes.InvalidArgument, "invalid custom launch proposal")
	}
	if (request.GetUri() == "") == (request.GetExecutableHandle() == "") {
		return nil, status.Error(codes.InvalidArgument, "a recipe names exactly one direct target")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.proposals = append(h.proposals, pbproto.Clone(request).(*proto.CustomLaunchRequest))
	return &proto.CustomLaunchReply{
		RecipeId:       "recipe-1",
		RecipeRevision: 1,
		Message:        "recipe saved; execution requires a separately approved request",
	}, nil
}

// GamesRequestCustomLaunch accepts only the reviewed recipe revision. The plan is shown to the
// user, who approves or declines; the reply carries a job that reports the outcome.
func (h *launchReviewHost) GamesRequestCustomLaunch(_ context.Context, request *proto.CustomLaunchRequest) (*proto.CustomLaunchReply, error) {
	if err := h.authorize(request.GetContext()); err != nil {
		return nil, err
	}
	if request.GetExecutableHandle() != "" || len(request.GetArgv()) != 0 ||
		request.GetWorkingDirectoryHandle() != "" || len(request.GetEnvironmentSet()) != 0 ||
		len(request.GetEnvironmentUnset()) != 0 || request.GetRuntimeHandle() != "" ||
		request.GetUri() != "" || request.GetContentId() != "" || len(request.GetModLayers()) != 0 {
		return nil, status.Error(codes.InvalidArgument, "custom execution must reference an approved recipe revision")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if len(h.proposals) == 0 || request.GetRecipeId() != "recipe-1" || request.GetRecipeRevision() != 1 {
		return nil, status.Error(codes.Aborted, "custom launch recipe revision changed")
	}
	approved := true
	if len(h.answers) > 0 {
		approved = h.answers[0]
		h.answers = h.answers[1:]
	}
	jobID := fmt.Sprintf("job-%d", len(h.requests)+1)
	h.requests = append(h.requests, pbproto.Clone(request).(*proto.CustomLaunchRequest))
	// The review shows the stored recipe, never the request's replacement fields.
	h.reviewedPlans = append(h.reviewedPlans, pbproto.Clone(h.proposals[len(h.proposals)-1]).(*proto.CustomLaunchRequest))
	job := &proto.JobReply{JobId: jobID, State: proto.JobState_JOB_STATE_QUEUED, Message: "awaiting confirmation"}
	if approved {
		h.launches++
		job.State = proto.JobState_JOB_STATE_SUCCEEDED
		job.ResultCode = "opened"
		job.Message = "launched the reviewed recipe"
	} else {
		job.State = proto.JobState_JOB_STATE_FAILED
		job.ResultCode = "declined"
		job.Message = "the user declined"
	}
	h.jobs[jobID] = job
	return &proto.CustomLaunchReply{
		RecipeId:       "recipe-1",
		RecipeRevision: 1,
		JobId:          jobID,
		State:          proto.JobState_JOB_STATE_QUEUED,
		Message:        "awaiting confirmation",
	}, nil
}

func (h *launchReviewHost) JobsGet(_ context.Context, request *proto.JobGetRequest) (*proto.JobReply, error) {
	if err := h.authorize(request.GetContext()); err != nil {
		return nil, err
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	job := h.jobs[request.GetJobId()]
	if job == nil {
		return nil, status.Error(codes.NotFound, "fixture job not found")
	}
	return pbproto.Clone(job).(*proto.JobReply), nil
}

// GamesRequestLaunch proves the denied custom request never falls back to standard launch.
func (h *launchReviewHost) GamesRequestLaunch(_ context.Context, _ *proto.GameLaunchRequest) (*proto.GameLaunchReply, error) {
	h.mu.Lock()
	h.launches++
	h.mu.Unlock()
	return nil, status.Error(codes.PermissionDenied, "the fixture does not allow standard launch")
}

func (h *launchReviewHost) counts() (proposals, requests, reviewed, launches int) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.proposals), len(h.requests), len(h.reviewedPlans), h.launches
}

// TestGeneratedExtensionClientReviewsAndRefusesCustomLaunch exercises the common launch review
// through the example's generated Go client: propose, receive the reviewed approval and request
// execution, then the declined and ungranted paths. The fixture host validates the extension
// context and answers with the same job contract the desktop host serves.
func TestGeneratedExtensionClientReviewsAndRefusesCustomLaunch(t *testing.T) {
	listener, endpoint := listenFixture(t)
	credential := []byte("0123456789abcdef0123456789abcdef")
	host := newLaunchReviewHost(credential, true, false)
	server := grpc.NewServer()
	proto.RegisterResourceHostServer(server, host)
	serveDone := make(chan error, 1)
	go func() { serveDone <- server.Serve(listener) }()
	t.Cleanup(func() {
		server.Stop()
		_ = listener.Close()
		select {
		case <-serveDone:
		case <-time.After(time.Second):
			t.Error("fixture gRPC server did not stop")
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	connection, err := dial(ctx, endpoint)
	if err != nil {
		t.Fatalf("connect to fixture host: %v", err)
	}
	t.Cleanup(func() { _ = connection.Close() })
	client := proto.NewResourceHostClient(connection)
	serviceContext := &proto.ServiceContext{
		SessionId:         "fixture-session",
		SessionCredential: append([]byte(nil), credential...),
		SessionKind:       proto.SessionKind_SESSION_KIND_EXTENSION,
		TargetGame: &proto.TargetGameContext{
			GameId:         "steam:620",
			InstallationId: "install-1",
			ProfileId:      "default",
			Revision:       "r7",
		},
	}

	proposal := &proto.CustomLaunchRequest{
		Context:                serviceContext,
		GameId:                 "620",
		InstallationId:         "install-1",
		ExpectedRevision:       12,
		IdempotencyKey:         "propose-reviewed-recipe",
		Uri:                    "steam://rungameid/620",
		Argv:                   []string{"--literal value", "$(not-a-shell)"},
		EnvironmentSet:         map[string]string{"FIXTURE_MODE": "reviewed"},
		EnvironmentUnset:       []string{"FIXTURE_LEGACY_MODE"},
		WorkingDirectoryHandle: "cwd-handle",
	}
	proposed, err := client.GamesProposeCustomLaunch(ctx, proposal)
	if err != nil {
		t.Fatalf("propose custom launch: %v", err)
	}
	if proposed.GetRecipeId() != "recipe-1" || proposed.GetRecipeRevision() != 1 {
		t.Fatalf("proposal reply = %v, want the stored recipe revision", proposed)
	}
	if proposed.GetJobId() != "" || proposed.GetState() != proto.JobState_JOB_STATE_UNSPECIFIED ||
		proposed.GetRefusalCode() != "" {
		t.Fatalf("a proposal must not run a job: %v", proposed)
	}
	if got, _, _, _ := host.counts(); got != 1 {
		t.Fatalf("stored proposals = %d, want 1", got)
	}
	host.mu.Lock()
	stored := pbproto.Clone(host.proposals[0]).(*proto.CustomLaunchRequest)
	host.mu.Unlock()
	if !reflect.DeepEqual(stored.GetArgv(), proposal.GetArgv()) ||
		!reflect.DeepEqual(stored.GetEnvironmentSet(), proposal.GetEnvironmentSet()) ||
		!reflect.DeepEqual(stored.GetEnvironmentUnset(), proposal.GetEnvironmentUnset()) ||
		stored.GetWorkingDirectoryHandle() != proposal.GetWorkingDirectoryHandle() {
		t.Fatalf("the host did not receive the reviewed launch fields: %v", stored)
	}

	// Execution cannot replace the fields the user reviewed: only the recipe reference is valid.
	if _, err := client.GamesRequestCustomLaunch(ctx, &proto.CustomLaunchRequest{
		Context:          serviceContext,
		GameId:           "620",
		InstallationId:   "install-1",
		ExpectedRevision: 12,
		IdempotencyKey:   "run-replaced-fields",
		RecipeId:         proposed.GetRecipeId(),
		RecipeRevision:   proposed.GetRecipeRevision(),
		Argv:             []string{"--replacement"},
	}); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("execution with replacement fields = %v, want INVALID_ARGUMENT", err)
	}

	// The user approves the review; execution names only the stored recipe revision.
	approved, err := client.GamesRequestCustomLaunch(ctx, &proto.CustomLaunchRequest{
		Context:          serviceContext,
		GameId:           "620",
		InstallationId:   "install-1",
		ExpectedRevision: 12,
		IdempotencyKey:   "run-reviewed-recipe",
		RecipeId:         proposed.GetRecipeId(),
		RecipeRevision:   proposed.GetRecipeRevision(),
	})
	if err != nil {
		t.Fatalf("request approved custom launch: %v", err)
	}
	if approved.GetState() != proto.JobState_JOB_STATE_QUEUED || approved.GetMessage() != "awaiting confirmation" {
		t.Fatalf("execution reply = %v, want the queued review job", approved)
	}
	finished, err := client.JobsGet(ctx, &proto.JobGetRequest{Context: serviceContext, JobId: approved.GetJobId()})
	if err != nil {
		t.Fatalf("read approved job: %v", err)
	}
	if finished.GetState() != proto.JobState_JOB_STATE_SUCCEEDED || finished.GetResultCode() != "opened" {
		t.Fatalf("approved job = %v, want succeeded/opened", finished)
	}

	// The user declines the next review: the job fails and nothing is dispatched.
	declined, err := client.GamesRequestCustomLaunch(ctx, &proto.CustomLaunchRequest{
		Context:          serviceContext,
		GameId:           "620",
		InstallationId:   "install-1",
		ExpectedRevision: 12,
		IdempotencyKey:   "run-declined-recipe",
		RecipeId:         proposed.GetRecipeId(),
		RecipeRevision:   proposed.GetRecipeRevision(),
	})
	if err != nil {
		t.Fatalf("request declined review: %v", err)
	}
	refused, err := client.JobsGet(ctx, &proto.JobGetRequest{Context: serviceContext, JobId: declined.GetJobId()})
	if err != nil {
		t.Fatalf("read declined job: %v", err)
	}
	if refused.GetState() != proto.JobState_JOB_STATE_FAILED || refused.GetResultCode() != "declined" {
		t.Fatalf("declined job = %v, want failed/declined", refused)
	}

	// Revoking the grant denies both the proposal and execution, with no standard-launch fallback.
	host.mu.Lock()
	host.granted = false
	host.mu.Unlock()
	if _, err := client.GamesProposeCustomLaunch(ctx, proposal); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("proposal without the grant = %v, want PERMISSION_DENIED", err)
	}
	if _, err := client.GamesRequestCustomLaunch(ctx, &proto.CustomLaunchRequest{
		Context:          serviceContext,
		GameId:           "620",
		InstallationId:   "install-1",
		ExpectedRevision: 12,
		IdempotencyKey:   "run-without-grant",
		RecipeId:         proposed.GetRecipeId(),
		RecipeRevision:   proposed.GetRecipeRevision(),
	}); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("execution without the grant = %v, want PERMISSION_DENIED", err)
	}

	proposals, requests, reviewed, launches := host.counts()
	if proposals != 1 || requests != 2 || reviewed != 2 || launches != 1 {
		t.Fatalf("fixture saw proposals=%d requests=%d reviews=%d dispatches=%d, want 1/2/2/1",
			proposals, requests, reviewed, launches)
	}
	host.mu.Lock()
	defer host.mu.Unlock()
	for index, plan := range host.reviewedPlans {
		if plan.GetRecipeId() != "" {
			t.Fatalf("review %d was the request, not the stored recipe", index)
		}
		if !reflect.DeepEqual(plan.GetArgv(), proposal.GetArgv()) ||
			plan.GetEnvironmentSet()["FIXTURE_MODE"] != "reviewed" {
			t.Fatalf("review %d did not describe the approved recipe: %v", index, plan)
		}
	}
}
