#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

CARGO=${CARGO:-cargo}
ENGINE_TEST_LIST=$("$CARGO" test -p rsproxy-engine --lib --locked -- --list)
NET_TEST_LIST=$("$CARGO" test -p rsproxy-net --lib --locked -- --list)
CASE_COUNT=0

fail() {
    printf 'protocol matrix: %s\n' "$*" >&2
    exit 1
}

run_owned_case() {
    case_id=$1
    package=$2
    test_name=$3

    case $package in
        rsproxy-engine) test_list=$ENGINE_TEST_LIST ;;
        rsproxy-net) test_list=$NET_TEST_LIST ;;
        *) fail "unknown test owner package for $case_id: $package" ;;
    esac

    if ! printf '%s\n' "$test_list" | grep -Fqx "$test_name: test"; then
        fail "missing test owner for $case_id in $package: $test_name"
    fi

    printf '\nprotocol_case=%s package=%s test=%s\n' "$case_id" "$package" "$test_name"
    "$CARGO" test -p "$package" --lib --locked "$test_name" -- --exact
    CASE_COUNT=$((CASE_COUNT + 1))
}

run_case() {
    run_owned_case "$1" rsproxy-engine "$2"
}

run_net_case() {
    run_owned_case "$1" rsproxy-net "$2"
}

# HTTP/1 admission, persistence, streaming, and authentication.
run_case h1.persistence \
    proxy::tests::connection::client_persistence_follows_http_version_and_connection_tokens
run_case h1.pipeline \
    proxy::tests::connection::client_connection_processes_pipelined_requests_in_order
run_case h1.expect-continue \
    proxy::tests::request_streaming::fixed::expect_continue_is_answered_before_the_streamed_body_is_read
run_case auth.reject-before-body \
    proxy::tests::request_streaming::fixed::proxy_auth_rejects_before_reading_or_acknowledging_the_body
run_case auth.strip-credentials \
    proxy::tests::connection::proxy_auth_credentials_are_stripped_before_dispatch

# CONNECT policy and protocol probing.
run_case connect.mitm \
    proxy::tests::connect_modes::tls_clienthello_still_enters_the_mitm_http_pipeline
run_case connect.passthrough \
    proxy::tests::connect_modes::no_mitm_passthrough_wins_even_when_a_ca_is_initialized
run_case connect.unknown-protocol \
    proxy::tests::connect_modes::auto_mode_passthroughs_unknown_protocol_after_non_consuming_probe

# HTTP/2 bridges, bounded duplex flow, and all implemented trailer directions.
run_case h2.client-to-h1-origin \
    proxy::tests::h2_tls::h2_bridge_reuses_rule_and_trace_pipeline
run_case h2.h1-client-to-h2-origin \
    proxy::tests::origin_h2_streaming::oversized_h1_upload_streams_to_h2_origin_with_trailers
run_case h2.h2-client-to-h2-origin \
    proxy::tests::origin_h2_streaming::oversized_h2_upload_streams_to_h2_origin_with_trailers
run_case h2.bounded-duplex \
    proxy::tests::h2_downstream_streaming::downstream_h2_request_and_response_stream_with_bounded_backpressure
run_case trailers.h1-client-to-h1-origin \
    proxy::tests::request_streaming::chunked::chunked_upload_streams_decoded_data_and_preserves_trailers
run_case trailers.response-actions \
    proxy::tests::response_actions::framing::response_trailer_actions_set_override_and_remove

# Request framing and large-body fallback boundaries.
run_net_case framing.reject-cl-te \
    http::tests::request_rejects_content_length_transfer_encoding_ambiguity
run_net_case framing.reject-forbidden-trailer \
    http::tests::chunked_request_rejects_framing_trailers
run_net_case framing.limit-trailer-count \
    http::tests::chunked_request_trailer_count_is_limited
run_case stream.large-fixed-keepalive \
    proxy::tests::request_streaming::fixed::large_fixed_upload_streams_and_preserves_client_keep_alive
run_case stream.slow-upload-deadline \
    proxy::tests::request_streaming::fixed::slow_streamed_upload_obeys_the_request_total_deadline
run_case stream.body-rule-limit \
    proxy::tests::request_streaming::rules::body_rules_apply_below_limit_and_skip_only_body_behavior_above_it

# Specialized protocols and TLS policy owners.
run_case grpc.response-trailers \
    proxy::tests::h2_tls::upstream_h2_response_reuses_response_rules_and_preserves_grpc_trailers
run_case websocket.frame-decoding \
    proxy::tests::websocket::websocket_reader_unmasks_client_frames_and_reads_fin
run_case websocket.trace-metadata \
    proxy::tests::websocket::websocket_trace_preserves_fragmentation_metadata
run_case websocket.network-duplex \
    proxy::tests::protocol_matrix::websocket::websocket_upgrade_forwards_server_first_and_client_frames_over_real_sockets
run_case sse.streaming \
    proxy::tests::response_actions::framing::streaming_sse_decodes_chunked_and_captures_frames
run_case mtls.origin-scope \
    proxy::tests::tls_policy::upstream_mtls_flag_only_applies_to_origin_tls_paths
run_case mtls.network-required-client-cert \
    proxy::tests::protocol_matrix::mtls::upstream_mtls_succeeds_with_client_identity_and_fails_without_it_over_real_tls
run_case tls.network-policy \
    proxy::tests::action_effects::tls::tls_family_reaches_the_upstream_handshake_with_selected_policy

# Header limits on the HTTP/1 and upstream HTTP/2 parsing paths.
run_case headers.h1-network-boundary \
    proxy::tests::protocol_matrix::headers::h1_large_header_accepts_200kb_and_rejects_over_limit_with_431
run_case headers.h2-network-boundary \
    proxy::tests::protocol_matrix::headers::h2_large_header_accepts_200kb_and_rejects_over_limit_with_431
run_net_case headers.h1-request-count \
    http::tests::request_header_count_limit_is_enforced
run_net_case headers.h1-response-count \
    http::tests::response_header_count_limit_is_enforced
run_net_case headers.h2-response-count \
    upstream_h2::tests::message::response_header_count_limit_includes_status_pseudo_header

# Address and authority normalization over real network routes.
run_case names.ipv6-and-punycode \
    proxy::tests::protocol_matrix::names::ipv6_literal_and_punycode_host_route_over_real_network_paths

printf '\nprotocol_matrix=ok cases=%s\n' "$CASE_COUNT"
