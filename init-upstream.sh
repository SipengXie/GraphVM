#!/bin/bash

if ! git remote | grep -q upstream; then
    git remote add upstream https://github.com/bluealloy/revm.git
fi
LOCAL_BRANCH=$(git rev-parse --abbrev-ref HEAD) 
git fetch upstream
git branch --set-upstream-to=upstream/release/v52 $LOCAL_BRANCH