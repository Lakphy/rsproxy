use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::proxy) enum UpstreamPlan {
    H1 { pooled: bool },
    H2 { pooled: bool, streaming: bool },
    WebSocket,
}

#[derive(Clone, Copy)]
struct PlanSignals {
    websocket: bool,
    h2_eligible: bool,
    h1_pool_eligible: bool,
    streaming: bool,
}

pub(in crate::proxy) fn plan_upstream(ctx: &ForwardCtx<'_>, streaming: bool) -> UpstreamPlan {
    choose(PlanSignals {
        websocket: ctx.websocket_request(),
        h2_eligible: !accepts_sse(&ctx.request.headers)
            && throttle_bps(ctx.actions, Phase::Req).is_none()
            && origin_tls_supported(ctx.url, ctx.route),
        h1_pool_eligible: !streaming && h1_forward::pool_eligible(ctx),
        streaming,
    })
}

fn choose(signals: PlanSignals) -> UpstreamPlan {
    if signals.websocket {
        UpstreamPlan::WebSocket
    } else if signals.h2_eligible {
        UpstreamPlan::H2 {
            pooled: true,
            streaming: signals.streaming,
        }
    } else {
        UpstreamPlan::H1 {
            pooled: signals.h1_pool_eligible,
        }
    }
}

#[cfg(test)]
#[path = "plan/tests.rs"]
mod tests;
