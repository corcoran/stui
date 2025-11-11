#!/usr/bin/env bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
print_step() {
    echo -e "\n${BLUE}==== $1 ====${NC}\n"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

confirm() {
    local prompt="$1"
    local response
    while true; do
        read -p "$prompt [y/n]: " response
        case "$response" in
            [Yy]* ) return 0;;
            [Nn]* ) return 1;;
            * ) echo "Please answer y or n.";;
        esac
    done
}

wait_for_github_actions() {
    # Check if gh CLI is available
    if ! command -v gh &> /dev/null; then
        print_warning "GitHub CLI (gh) not found. Install it to auto-monitor workflows."
        echo "  See: https://cli.github.com/"
        return 0
    fi

    echo ""
    if ! confirm "Wait for GitHub Actions to complete?"; then
        return 0
    fi

    print_step "Monitoring GitHub Actions"

    # Get the commit SHA
    commit_sha=$(git rev-parse HEAD)
    echo "Repository: $repo"
    echo "Commit: $commit_sha"
    echo "Checking for workflow runs..."

    # Wait a moment for GitHub to register the push
    sleep 3

    # Poll for workflow status
    local max_attempts=60  # 5 minutes (60 * 5 seconds)
    local attempt=0
    local found_workflow=false

    while [ $attempt -lt $max_attempts ]; do
        # Get workflow runs for this commit (don't exit on error due to set -e)
        local status
        status=$(gh run list --repo "$repo" --commit "$commit_sha" --json status,conclusion,name --jq '.[0] | {status: .status, conclusion: .conclusion, name: .name}' 2>/dev/null) || true

        if [ -n "$status" ] && [ "$status" != "null" ] && [ "$status" != "{}" ]; then
            local workflow_status
            local workflow_name
            local conclusion
            workflow_status=$(echo "$status" | jq -r '.status // empty')
            workflow_name=$(echo "$status" | jq -r '.name // empty')

            if [ -z "$workflow_status" ] || [ "$workflow_status" = "null" ]; then
                echo -ne "\rWaiting for workflow to start... ($((attempt * 5))s)  "
            elif [ "$workflow_status" = "completed" ]; then
                conclusion=$(echo "$status" | jq -r '.conclusion')
                echo ""  # New line after the waiting message
                if [ "$conclusion" = "success" ]; then
                    print_success "Workflow '$workflow_name' passed!"
                    return 0
                else
                    print_error "Workflow '$workflow_name' failed with conclusion: $conclusion"
                    echo ""
                    echo "View details: https://github.com/$repo/actions"
                    exit 1
                fi
            else
                found_workflow=true
                echo -ne "\rWaiting for workflow '$workflow_name' to complete... ($((attempt * 5))s)  "
            fi
        else
            echo -ne "\rWaiting for workflow to start... ($((attempt * 5))s)  "
        fi

        sleep 5 || true
        attempt=$((attempt + 1)) || true
    done

    echo ""  # Ensure we're on a new line after the loop
    echo "DEBUG: Loop completed after $attempt attempts"

    if [ $attempt -eq $max_attempts ]; then
        print_warning "Timeout waiting for workflow. Check manually:"
        echo "  https://github.com/$repo/actions"
        if ! confirm "Continue anyway?"; then
            exit 1
        fi
    fi
    echo ""
}

# Check we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    print_error "Cargo.toml not found. Please run this script from the project root."
    exit 1
fi

# Check we're on master branch
current_branch=$(git branch --show-current)
if [ "$current_branch" != "master" ]; then
    print_warning "Current branch is '$current_branch', not 'master'"
    if ! confirm "Continue anyway?"; then
        exit 1
    fi
fi

# Check for uncommitted changes
if ! git diff-index --quiet HEAD --; then
    print_error "You have uncommitted changes. Please commit or stash them first."
    git status --short
    exit 1
fi

# Extract repo from git remote (for use in URLs)
remote_url=$(git remote get-url origin)
if [[ "$remote_url" =~ github\.com[:/]([^/]+/[^/]+)(\.git)?$ ]]; then
    repo="${BASH_REMATCH[1]}"
    repo="${repo%.git}"  # Remove .git suffix if present
else
    print_error "Could not parse GitHub repository from remote URL: $remote_url"
    print_error "Expected format: git@github.com:owner/repo.git or https://github.com/owner/repo.git"
    exit 1
fi

print_step "Step 1: Get New Version Number"

# Get current version from Cargo.toml
current_version=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $current_version"
echo ""
read -p "Enter new version number (e.g., 0.10.0): " new_version

if [ -z "$new_version" ]; then
    print_error "Version number cannot be empty"
    exit 1
fi

# Validate version format (basic check)
if ! [[ "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    print_error "Invalid version format. Expected: X.Y.Z (e.g., 0.10.0)"
    exit 1
fi

echo ""
if [ "$current_version" = "$new_version" ]; then
    print_warning "Version is already set to $new_version in Cargo.toml"
    echo "This appears to be a re-release attempt."
    if ! confirm "Continue with re-release?"; then
        exit 1
    fi
    is_rerelease=true
else
    echo "Version change: $current_version → $new_version"
    if ! confirm "Is this correct?"; then
        exit 1
    fi
    is_rerelease=false
fi

if [ "$is_rerelease" = false ]; then
    print_step "Step 2: Update Cargo.toml"

    # Update version in Cargo.toml
    sed -i.bak "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml
    rm Cargo.toml.bak
    print_success "Updated Cargo.toml to version $new_version"
else
    print_step "Step 2: Update Cargo.toml"
    print_success "Skipped - version already set to $new_version"
fi

print_step "Step 3: Test Build"

echo "Running cargo build --release..."
if cargo build --release; then
    print_success "Build succeeded"
else
    print_error "Build failed"
    # Restore original version
    git checkout Cargo.toml
    exit 1
fi

echo ""
echo "Running cargo test..."
if cargo test; then
    print_success "All tests passed"
else
    print_error "Tests failed"
    # Restore original version
    git checkout Cargo.toml Cargo.lock
    exit 1
fi

if [ "$is_rerelease" = false ]; then
    print_step "Step 4: Commit Version Bump"

    # Show what will be committed
    echo "Changes to commit:"
    git diff Cargo.toml Cargo.lock

    echo ""
    if ! confirm "Commit these changes?"; then
        git checkout Cargo.toml Cargo.lock
        exit 1
    fi

    git add Cargo.toml Cargo.lock
    git commit -m "chore: Bump version to $new_version"
    print_success "Committed version bump"

    echo ""
    if confirm "Push to origin $current_branch?"; then
        git push origin "$current_branch"
        print_success "Pushed to origin/$current_branch"
        wait_for_github_actions
    else
        print_warning "Skipped push. Remember to push before creating tag!"
    fi
else
    print_step "Step 4: Commit Version Bump"
    print_success "Skipped - re-releasing existing version"

    # For re-releases, check if tests passed on current commit or recent commits
    if command -v gh &> /dev/null; then
        commit_sha=$(git rev-parse HEAD)
        echo ""
        echo "Checking test status for current commit: $commit_sha"

        # Check if a workflow run exists for this commit
        status=$(gh run list --repo "$repo" --commit "$commit_sha" --json status,conclusion,name,headSha --jq '.[0] | {status: .status, conclusion: .conclusion, name: .name, headSha: .headSha}' 2>/dev/null) || true

        if [ -n "$status" ] && [ "$status" != "null" ] && [ "$status" != "{}" ]; then
            workflow_status=$(echo "$status" | jq -r '.status // empty')
            workflow_name=$(echo "$status" | jq -r '.name // empty')
            conclusion=$(echo "$status" | jq -r '.conclusion // empty')

            # Check if we actually got valid workflow data
            if [ -z "$workflow_status" ] || [ "$workflow_status" = "null" ]; then
                # Empty response, treat as no workflow found
                status=""
            elif [ "$workflow_status" = "completed" ]; then
                if [ "$conclusion" = "success" ]; then
                    print_success "Workflow '$workflow_name' already passed for this commit"
                else
                    print_error "Workflow '$workflow_name' failed for this commit (conclusion: $conclusion)"
                    echo "View details: https://github.com/$repo/actions"
                    if ! confirm "Continue anyway?"; then
                        exit 1
                    fi
                fi
            else
                print_warning "Workflow '$workflow_name' is still running ($workflow_status)"
                # Still running, offer to wait
                wait_for_github_actions
            fi
        fi

        if [ -z "$status" ] || [ "$status" = "null" ] || [ "$status" = "{}" ]; then
            # No workflow on HEAD, check for most recent workflow run on this branch
            print_warning "No workflow runs found for current commit (may be docs-only change)"
            echo "Checking for most recent workflow run on branch $current_branch..."

            recent_run=$(gh run list --repo "$repo" --branch "$current_branch" --limit 1 --json status,conclusion,name,headSha,createdAt --jq '.[0] | {status: .status, conclusion: .conclusion, name: .name, headSha: .headSha, createdAt: .createdAt}' 2>/dev/null) || true

            if [ -n "$recent_run" ] && [ "$recent_run" != "null" ] && [ "$recent_run" != "{}" ]; then
                recent_status=$(echo "$recent_run" | jq -r '.status // empty')
                recent_name=$(echo "$recent_run" | jq -r '.name // empty')
                recent_conclusion=$(echo "$recent_run" | jq -r '.conclusion // empty')
                recent_sha=$(echo "$recent_run" | jq -r '.headSha // empty')
                recent_short_sha="${recent_sha:0:7}"

                echo "Found workflow '$recent_name' on commit $recent_short_sha"

                if [ "$recent_status" = "completed" ] && [ "$recent_conclusion" = "success" ]; then
                    print_success "Most recent workflow passed"
                    if ! confirm "Use this workflow result for release verification?"; then
                        exit 1
                    fi
                else
                    print_error "Most recent workflow status: $recent_status (conclusion: $recent_conclusion)"
                    echo "View details: https://github.com/$repo/actions"
                    if ! confirm "Continue anyway?"; then
                        exit 1
                    fi
                fi
            else
                print_warning "No workflow runs found on branch $current_branch"
                if confirm "Continue without test verification?"; then
                    print_warning "Proceeding without test verification"
                else
                    exit 1
                fi
            fi
        fi
    else
        print_warning "GitHub CLI (gh) not found. Skipping test verification."
        echo "  Install gh to verify tests: https://cli.github.com/"
    fi
fi

print_step "Step 5: Create and Push Tag"

tag_name="v$new_version"
echo "Creating tag: $tag_name"

# Check if tag already exists
if git tag -l | grep -q "^$tag_name$"; then
    print_warning "Tag $tag_name already exists"
    if confirm "Delete existing tag and recreate?"; then
        # Delete local tag
        git tag -d "$tag_name"
        print_success "Deleted local tag $tag_name"

        # Delete remote tag if it exists
        if git ls-remote --tags origin | grep -q "refs/tags/$tag_name"; then
            git push origin ":refs/tags/$tag_name"
            print_success "Deleted remote tag $tag_name"
        fi

        # Delete GitHub release if it exists (requires gh CLI)
        if command -v gh &> /dev/null; then
            if gh release view "$tag_name" --repo "$repo" &>/dev/null; then
                print_warning "GitHub release $tag_name exists"
                if confirm "Delete existing GitHub release?"; then
                    gh release delete "$tag_name" --repo "$repo" --yes
                    print_success "Deleted GitHub release $tag_name"
                else
                    print_warning "Release not deleted. The workflow may fail when trying to create it."
                fi
            fi
        fi
    else
        print_warning "Tag not deleted. Exiting."
        exit 0
    fi
fi

if ! confirm "Create tag $tag_name?"; then
    exit 1
fi

git tag "$tag_name"
print_success "Created tag $tag_name"

echo ""
if ! confirm "Push tag to origin? (This triggers the release workflow)"; then
    print_warning "Tag created locally but not pushed"
    echo "To push later: git push origin $tag_name"
    exit 0
fi

git push origin "$tag_name"
print_success "Pushed tag $tag_name"

print_step "Next Steps"

echo "✓ Version bump committed and tag pushed"
echo ""
echo "Monitor the release workflow at:"
echo "  https://github.com/$repo/actions"
echo ""
echo "Expected workflow:"
echo "  - 5 parallel build jobs (Linux x86_64/ARM64, macOS Intel/ARM, Windows)"
echo "  - 1 release job (creates draft release)"
echo "  - Build time: ~3 minutes"
echo ""
echo "After workflow completes:"
echo "  1. Review draft release: https://github.com/$repo/releases"
echo "  2. Edit release notes (optional)"
echo "  3. Publish release"
echo ""
print_success "Release process complete!"
