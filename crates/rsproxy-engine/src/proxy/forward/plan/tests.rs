use super::*;

#[test]
fn decision_table_covers_protocol_pooling_streaming_and_websocket() {
    for websocket in [false, true] {
        for h2_eligible in [false, true] {
            for h1_pool_eligible in [false, true] {
                for streaming in [false, true] {
                    let plan = choose(PlanSignals {
                        websocket,
                        h2_eligible,
                        h1_pool_eligible,
                        streaming,
                    });
                    let expected = if websocket {
                        UpstreamPlan::WebSocket
                    } else if h2_eligible {
                        UpstreamPlan::H2 {
                            pooled: true,
                            streaming,
                        }
                    } else {
                        UpstreamPlan::H1 {
                            pooled: h1_pool_eligible,
                        }
                    };
                    assert_eq!(plan, expected);
                }
            }
        }
    }
}
