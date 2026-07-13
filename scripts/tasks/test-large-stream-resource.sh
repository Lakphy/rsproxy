#!/usr/bin/env bash
set -euo pipefail

export RSPROXY_LARGE_STREAM_BYTES="${RSPROXY_LARGE_STREAM_BYTES:-1073741824}"
export RSPROXY_LARGE_STREAM_MAX_RSS_GROWTH_MB="${RSPROXY_LARGE_STREAM_MAX_RSS_GROWTH_MB:-96}"

cargo test -p rsproxy-cli --release --test large_stream_resource \
  one_gib_proxy_stream_has_bounded_rss_and_exact_trace -- \
  --ignored --exact --nocapture
