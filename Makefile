.PHONY: update-tags docker-build

IMAGE_TAG ?= action-pull-request-merge

# Supported platforms. Rust cross-compile needs a per-arch rustup target;
# amd64 is the default here. Override PLATFORM to build/push others.
PLATFORM ?= linux/amd64

ACTION ?= load
PROGRESS_MODE ?= plain

docker-build:
	# https://github.com/docker/buildx#building
	docker buildx build \
		--tag $(IMAGE_TAG) \
		--progress $(PROGRESS_MODE) \
		--platform $(PLATFORM) \
		--file docker/Dockerfile \
		--build-arg VCS_REF=`git rev-parse HEAD` \
		--build-arg BUILD_DATE=`date -u +"%Y-%m-%dT%H:%M:%SZ"` \
		--$(ACTION) \
		.

update-tags:
	git checkout main
	git tag -s -f -a -m "latest series" latest
	git checkout -
	git push origin refs/tags/latest -f
