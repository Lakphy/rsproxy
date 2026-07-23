use super::*;

const REQUEST_PHASES: &[Phase] = &[Phase::Req];
const RESPONSE_PHASES: &[Phase] = &[Phase::Res];
const BOTH_PHASES: &[Phase] = &[Phase::Req, Phase::Res];

/// Resolution behavior shared by every action in one family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionPolicy {
    /// Only the first applicable action is retained.
    FirstWins,
    /// Every applicable action is retained in resolution order.
    Stackable,
}

macro_rules! action_families {
    ($( $variant:ident => ($name:literal, $phases:ident, $policy:ident) ),+ $(,)?) => {
        /// Stable typed identity of an action family.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum ActionFamily {
            $(
                #[doc = concat!("The `", $name, "` action family.")]
                $variant,
            )+
        }

        impl ActionFamily {
            /// Complete action-family set in stable language order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Returns the stable dotted family identifier used by diagnostics and help.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            /// Returns the phases in which this family can have an effect.
            pub const fn phases(self) -> &'static [Phase] {
                match self {
                    $(Self::$variant => $phases),+
                }
            }

            /// Returns the family's first-wins or stackable resolution policy.
            pub const fn resolution(self) -> ResolutionPolicy {
                match self {
                    $(Self::$variant => ResolutionPolicy::$policy),+
                }
            }

            /// Parses one canonical dotted family identifier.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub(crate) const fn bit(self) -> u64 {
                1u64 << self as u8
            }
        }
    };
}

action_families! {
    Host => ("host", REQUEST_PHASES, FirstWins),
    Upstream => ("upstream", REQUEST_PHASES, FirstWins),
    Direct => ("direct", REQUEST_PHASES, FirstWins),
    Mock => ("mock", REQUEST_PHASES, FirstWins),
    MapRemote => ("map.remote", REQUEST_PHASES, FirstWins),
    Status => ("status", REQUEST_PHASES, FirstWins),
    Redirect => ("redirect", REQUEST_PHASES, FirstWins),
    ReqHeader => ("req.header", REQUEST_PHASES, Stackable),
    ResHeader => ("res.header", RESPONSE_PHASES, Stackable),
    ResStatus => ("res.status", RESPONSE_PHASES, FirstWins),
    ReqMethod => ("req.method", REQUEST_PHASES, FirstWins),
    ReqCookie => ("req.cookie", REQUEST_PHASES, Stackable),
    ResCookie => ("res.cookie", RESPONSE_PHASES, Stackable),
    ReqUa => ("req.ua", REQUEST_PHASES, FirstWins),
    ReqReferer => ("req.referer", REQUEST_PHASES, FirstWins),
    ReqAuth => ("req.auth", REQUEST_PHASES, FirstWins),
    ReqForwarded => ("req.forwarded", REQUEST_PHASES, FirstWins),
    ReqType => ("req.type", REQUEST_PHASES, FirstWins),
    ReqCharset => ("req.charset", REQUEST_PHASES, FirstWins),
    ResCors => ("res.cors", RESPONSE_PHASES, FirstWins),
    ResType => ("res.type", RESPONSE_PHASES, FirstWins),
    ResCharset => ("res.charset", RESPONSE_PHASES, FirstWins),
    ResMerge => ("res.merge", RESPONSE_PHASES, Stackable),
    ResTrailer => ("res.trailer", RESPONSE_PHASES, Stackable),
    Attachment => ("attachment", RESPONSE_PHASES, FirstWins),
    Cache => ("cache", RESPONSE_PHASES, FirstWins),
    Tls => ("tls", REQUEST_PHASES, FirstWins),
    UrlRewrite => ("url.rewrite", REQUEST_PHASES, FirstWins),
    UrlQuery => ("url.query", REQUEST_PHASES, Stackable),
    Delete => ("delete", BOTH_PHASES, Stackable),
    ReqBodySet => ("req.body.set", REQUEST_PHASES, Stackable),
    ReqBodyPrepend => ("req.body.prepend", REQUEST_PHASES, Stackable),
    ReqBodyAppend => ("req.body.append", REQUEST_PHASES, Stackable),
    ReqBodyReplace => ("req.body.replace", REQUEST_PHASES, Stackable),
    ResBodySet => ("res.body.set", RESPONSE_PHASES, Stackable),
    ResBodyPrepend => ("res.body.prepend", RESPONSE_PHASES, Stackable),
    ResBodyAppend => ("res.body.append", RESPONSE_PHASES, Stackable),
    ResBodyReplace => ("res.body.replace", RESPONSE_PHASES, Stackable),
    Inject => ("inject", RESPONSE_PHASES, Stackable),
    DelayReq => ("delay.req", REQUEST_PHASES, FirstWins),
    DelayRes => ("delay.res", RESPONSE_PHASES, FirstWins),
    ThrottleReq => ("throttle.req", REQUEST_PHASES, FirstWins),
    ThrottleRes => ("throttle.res", RESPONSE_PHASES, FirstWins),
    Bypass => ("bypass", REQUEST_PHASES, FirstWins),
    Hide => ("hide", BOTH_PHASES, FirstWins),
    Tag => ("tag", BOTH_PHASES, Stackable),
    Skip => ("skip", BOTH_PHASES, Stackable),
}

const _: () = assert!(ActionFamily::ALL.len() <= u64::BITS as usize);

/// Compact set of action families used by `skip(...)` and resolver state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionFamilySet(u64);

impl ActionFamilySet {
    /// Empty family set.
    pub const EMPTY: Self = Self(0);
    /// Complete family set.
    pub const ALL: Self = Self(if ActionFamily::ALL.len() == u64::BITS as usize {
        u64::MAX
    } else {
        (1u64 << ActionFamily::ALL.len()) - 1
    });

    /// Reports whether the set contains one family.
    pub const fn contains(self, family: ActionFamily) -> bool {
        self.0 & family.bit() != 0
    }

    /// Adds one family to the set.
    pub fn insert(&mut self, family: ActionFamily) {
        self.0 |= family.bit();
    }

    /// Reports whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Iterates over contained families in stable language order.
    pub fn iter(self) -> impl Iterator<Item = ActionFamily> {
        ActionFamily::ALL
            .iter()
            .copied()
            .filter(move |family| self.contains(*family))
    }

    pub(crate) fn from_prefix(prefix: &str) -> Option<Self> {
        if matches!(prefix, "all" | "*") {
            return Some(Self::ALL);
        }
        let mut set = Self::EMPTY;
        for family in ActionFamily::ALL {
            if family_within(family.as_str(), prefix) {
                set.insert(*family);
            }
        }
        (!set.is_empty()).then_some(set)
    }

    pub(crate) fn union(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl std::fmt::Display for ActionFamily {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromIterator<ActionFamily> for ActionFamilySet {
    fn from_iter<T: IntoIterator<Item = ActionFamily>>(families: T) -> Self {
        let mut set = Self::EMPTY;
        for family in families {
            set.insert(family);
        }
        set
    }
}

impl Action {
    /// Complete stable set of action-family identifiers used by contracts and `skip(...)`.
    ///
    /// Body and phase-sensitive operations have distinct family names even when
    /// represented by the same Rust variant.
    pub const FAMILIES: &'static [ActionFamily] = ActionFamily::ALL;

    /// Stable set of action families that retain every applicable action.
    pub const STACKABLE_FAMILIES: &'static [ActionFamily] = &[
        ActionFamily::ReqHeader,
        ActionFamily::ResHeader,
        ActionFamily::ReqCookie,
        ActionFamily::ResCookie,
        ActionFamily::ResMerge,
        ActionFamily::ResTrailer,
        ActionFamily::UrlQuery,
        ActionFamily::Delete,
        ActionFamily::ReqBodySet,
        ActionFamily::ReqBodyPrepend,
        ActionFamily::ReqBodyAppend,
        ActionFamily::ReqBodyReplace,
        ActionFamily::ResBodySet,
        ActionFamily::ResBodyPrepend,
        ActionFamily::ResBodyAppend,
        ActionFamily::ResBodyReplace,
        ActionFamily::Inject,
        ActionFamily::Tag,
        ActionFamily::Skip,
    ];
}

impl Action {
    /// Returns this action's stable family identifier for ordering, skipping, and traces.
    pub fn family(&self) -> ActionFamily {
        match self {
            Action::Host(_) => ActionFamily::Host,
            Action::Upstream(_) => ActionFamily::Upstream,
            Action::Direct => ActionFamily::Direct,
            Action::Mock(_) | Action::MockRaw(_) | Action::MockInline(_) => ActionFamily::Mock,
            Action::MapRemote(_) => ActionFamily::MapRemote,
            Action::Status(_) => ActionFamily::Status,
            Action::Redirect { .. } => ActionFamily::Redirect,
            Action::ReqHeader(_) => ActionFamily::ReqHeader,
            Action::ResHeader(_) => ActionFamily::ResHeader,
            Action::ResStatus(_) => ActionFamily::ResStatus,
            Action::ReqMethod(_) => ActionFamily::ReqMethod,
            Action::ReqCookie(_) => ActionFamily::ReqCookie,
            Action::ResCookie(_) => ActionFamily::ResCookie,
            Action::ReqUa(_) => ActionFamily::ReqUa,
            Action::ReqReferer(_) => ActionFamily::ReqReferer,
            Action::ReqAuth(_) => ActionFamily::ReqAuth,
            Action::ReqForwarded(_) => ActionFamily::ReqForwarded,
            Action::ReqType(_) => ActionFamily::ReqType,
            Action::ReqCharset(_) => ActionFamily::ReqCharset,
            Action::ResCors(_) => ActionFamily::ResCors,
            Action::ResType(_) => ActionFamily::ResType,
            Action::ResCharset(_) => ActionFamily::ResCharset,
            Action::ResMerge(_) => ActionFamily::ResMerge,
            Action::ResTrailer(_) => ActionFamily::ResTrailer,
            Action::Attachment(_) => ActionFamily::Attachment,
            Action::Cache(_) => ActionFamily::Cache,
            Action::Tls(_) => ActionFamily::Tls,
            Action::UrlRewrite { .. } => ActionFamily::UrlRewrite,
            Action::UrlQuery(_) => ActionFamily::UrlQuery,
            Action::Delete(_) => ActionFamily::Delete,
            Action::ReqBody(BodyOp::Set(_)) => ActionFamily::ReqBodySet,
            Action::ReqBody(BodyOp::Prepend(_)) => ActionFamily::ReqBodyPrepend,
            Action::ReqBody(BodyOp::Append(_)) => ActionFamily::ReqBodyAppend,
            Action::ReqBody(BodyOp::Replace { .. }) => ActionFamily::ReqBodyReplace,
            Action::ResBody(BodyOp::Set(_)) => ActionFamily::ResBodySet,
            Action::ResBody(BodyOp::Prepend(_)) => ActionFamily::ResBodyPrepend,
            Action::ResBody(BodyOp::Append(_)) => ActionFamily::ResBodyAppend,
            Action::ResBody(BodyOp::Replace { .. }) => ActionFamily::ResBodyReplace,
            Action::Inject(_) => ActionFamily::Inject,
            Action::Delay {
                phase: Phase::Req, ..
            } => ActionFamily::DelayReq,
            Action::Delay {
                phase: Phase::Res, ..
            } => ActionFamily::DelayRes,
            Action::Throttle {
                phase: Phase::Req, ..
            } => ActionFamily::ThrottleReq,
            Action::Throttle {
                phase: Phase::Res, ..
            } => ActionFamily::ThrottleRes,
            Action::Bypass => ActionFamily::Bypass,
            Action::Hide => ActionFamily::Hide,
            Action::Tag(_) => ActionFamily::Tag,
            Action::Skip(_) => ActionFamily::Skip,
        }
    }

    pub(crate) fn is_single(&self) -> bool {
        !self.is_stackable()
    }

    /// Reports whether every applicable action in this action's family is retained.
    pub fn is_stackable(&self) -> bool {
        self.family().resolution() == ResolutionPolicy::Stackable
    }

    /// Reports whether this concrete action has an effect in the requested phase.
    ///
    /// A `delete(...)` action is classified from its typed operations; other
    /// actions use their stable family contract.
    pub fn applies_in(&self, phase: Phase) -> bool {
        match self {
            Action::Delete(operations) => operations
                .iter()
                .any(|operation| delete_operation_applies_in(operation, phase)),
            _ => self.family().phases().contains(&phase),
        }
    }

    /// Reports whether a stable action-family identifier has stackable semantics.
    pub fn family_is_stackable(family: ActionFamily) -> bool {
        family.resolution() == ResolutionPolicy::Stackable
    }

    /// Returns the phases in which a stable action family can have an effect.
    ///
    /// Unknown family identifiers return an empty slice. The `delete` family is
    /// phase-dependent; individual [`Action::Delete`] values may use only one
    /// of the two phases.
    pub const fn family_phases(family: ActionFamily) -> &'static [Phase] {
        family.phases()
    }

    /// Reports whether a stable action family can have an effect in one phase.
    pub fn family_applies_in(family: ActionFamily, phase: Phase) -> bool {
        family.phases().contains(&phase)
    }
}

impl Phase {
    /// Returns the stable machine-readable phase identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Req => "request",
            Self::Res => "response",
        }
    }
}

fn delete_operation_applies_in(operation: &DeleteOp, phase: Phase) -> bool {
    match operation {
        DeleteOp::Pathname
        | DeleteOp::PathSegment(_)
        | DeleteOp::UrlParams
        | DeleteOp::UrlParam(_)
        | DeleteOp::ReqHeader(_)
        | DeleteOp::ReqBody
        | DeleteOp::ReqBodyPath(_)
        | DeleteOp::ReqType
        | DeleteOp::ReqCharset
        | DeleteOp::ReqCookie(_)
        | DeleteOp::ReqCookies => phase == Phase::Req,
        DeleteOp::ResHeader(_)
        | DeleteOp::ResBody
        | DeleteOp::ResBodyPath(_)
        | DeleteOp::ResType
        | DeleteOp::ResCharset
        | DeleteOp::ResCookie(_)
        | DeleteOp::ResCookies
        | DeleteOp::Trailer(_)
        | DeleteOp::Trailers => phase == Phase::Res,
    }
}

impl InjectTarget {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Js => "js",
            Self::Css => "css",
        }
    }
}

impl InjectMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Prepend => "prepend",
            Self::Replace => "replace",
        }
    }
}
