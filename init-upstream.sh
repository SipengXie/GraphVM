#!/bin/bash

# Configuration variables
REMOTE_NAME="upstream"  # Remote repository name
LOCAL_BRANCH=$(git rev-parse --abbrev-ref HEAD)  # Local branch name
echo "Local branch name: $LOCAL_BRANCH"
REVM_REPO="https://github.com/bluealloy/revm.git"  # Upstream repository URL

# Exit immediately if a command exits with a non-zero status
set -e

# Check if the upstream remote is already configured
if ! git remote | grep -q $REMOTE_NAME; then
  echo "Adding upstream remote repository..."
  git remote add $REMOTE_NAME $REVM_REPO
else
  echo "Upstream remote repository already exists."
fi

# Fetch the latest tag
echo "Fetching the latest tag..."
LATEST_TAG=$(git ls-remote --tags $REMOTE_NAME | awk -F/ '{print $3}' | grep -v '{}' | sort -V | tail -n 1)

if [ -z "$LATEST_TAG" ]; then
  echo "No tags found."
  exit 1
fi

echo "The latest tag is: $LATEST_TAG"

# Generate branch name dynamically
BRANCH_NAME="release-$LATEST_TAG"

# Check if the branch already exists
if git show-ref --quiet refs/heads/$BRANCH_NAME; then
  echo "Branch $BRANCH_NAME already exists, skipping creation."
else
  # Fetch the upstream tag
  echo "Fetching tag $LATEST_TAG..."
  git fetch $REMOTE_NAME tag $LATEST_TAG

  # Create a new branch based on the latest tag
  echo "Creating branch: $BRANCH_NAME based on tag: $LATEST_TAG..."
  git branch $BRANCH_NAME $LATEST_TAG

  echo "Branch $BRANCH_NAME successfully created based on tag: $LATEST_TAG."
fi

echo "You can use the merge or rebase command to integrate $BRANCH_NAME into the current branch to track the latest revm version."

echo "Operation completed!"