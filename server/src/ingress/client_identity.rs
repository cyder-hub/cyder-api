use std::{
    fmt,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::HeaderMap,
    middleware::Next,
    response::{IntoResponse, Response},
};
use ipnet::IpNet;

use crate::{
    config::ClientIdentityConfig,
    proxy::{
        ExecutionStage, ProtocolErrorResponseAdapter, ProxyError, ProxyErrorCode,
        ProxyRequestContext, ResponseVisibility,
    },
    schema::enum_def::DownstreamProtocol,
};

const FORWARDED: &str = "forwarded";
const X_FORWARDED_FOR: &str = "x-forwarded-for";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIdentitySource {
    TcpPeer,
    Forwarded,
    XForwardedFor,
    ConsistentForwardedPair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    pub client_ip: IpAddr,
    pub peer_addr: SocketAddr,
    pub source: ClientIdentitySource,
    pub trusted_proxy_hops: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardingHeaderKind {
    Forwarded,
    XForwardedFor,
    Both,
}

impl ForwardingHeaderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forwarded => FORWARDED,
            Self::XForwardedFor => X_FORWARDED_FOR,
            Self::Both => "forwarded+x-forwarded-for",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIdentityErrorReason {
    InvalidHeaderEncoding,
    InvalidForwardedSyntax,
    ForwardedMissingFor,
    ForwardedDuplicateFor,
    ForwardedNonIpFor,
    InvalidXForwardedFor,
    ConflictingForwardingHeaders,
    ForwardedChainTooLong,
}

impl ClientIdentityErrorReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidHeaderEncoding => "invalid_header_encoding",
            Self::InvalidForwardedSyntax | Self::ForwardedMissingFor => "malformed_forwarded",
            Self::ForwardedDuplicateFor => "duplicate_for",
            Self::ForwardedNonIpFor => "unsupported_identifier",
            Self::InvalidXForwardedFor => "malformed_x_forwarded_for",
            Self::ConflictingForwardingHeaders => "conflicting_forwarded_chains",
            Self::ForwardedChainTooLong => "chain_too_long",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientIdentityError {
    pub reason: ClientIdentityErrorReason,
    pub header_kind: ForwardingHeaderKind,
    pub observed_hops: Option<usize>,
}

impl ClientIdentityError {
    fn new(reason: ClientIdentityErrorReason, header_kind: ForwardingHeaderKind) -> Self {
        Self {
            reason,
            header_kind,
            observed_hops: None,
        }
    }

    fn with_hops(
        reason: ClientIdentityErrorReason,
        header_kind: ForwardingHeaderKind,
        observed_hops: usize,
    ) -> Self {
        Self {
            reason,
            header_kind,
            observed_hops: Some(observed_hops),
        }
    }
}

impl fmt::Debug for ClientIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientIdentityError")
            .field("reason", &self.reason.as_str())
            .field("header_kind", &self.header_kind.as_str())
            .field("observed_hops", &self.observed_hops)
            .finish()
    }
}

impl fmt::Display for ClientIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason.as_str())
    }
}

impl std::error::Error for ClientIdentityError {}

#[derive(Debug, Clone)]
pub struct ClientIdentityResolver {
    trusted_proxy_cidrs: Vec<IpNet>,
    max_forwarded_hops: usize,
}

impl ClientIdentityResolver {
    pub fn new(config: &ClientIdentityConfig) -> Self {
        Self {
            trusted_proxy_cidrs: config.trusted_proxy_cidrs.clone(),
            max_forwarded_hops: config.max_forwarded_hops,
        }
    }

    pub fn resolve(
        &self,
        peer_addr: SocketAddr,
        headers: &HeaderMap,
    ) -> Result<ClientIdentity, ClientIdentityError> {
        let peer_addr = normalize_socket_addr(peer_addr);
        let peer_ip = peer_addr.ip();

        // Forwarding metadata has no authority until the transport peer is trusted.
        if !self.is_trusted(peer_ip) {
            return Ok(ClientIdentity {
                client_ip: peer_ip,
                peer_addr,
                source: ClientIdentitySource::TcpPeer,
                trusted_proxy_hops: 0,
            });
        }

        let has_forwarded = headers.contains_key(FORWARDED);
        let has_x_forwarded_for = headers.contains_key(X_FORWARDED_FOR);
        if !has_forwarded && !has_x_forwarded_for {
            return Ok(ClientIdentity {
                client_ip: peer_ip,
                peer_addr,
                source: ClientIdentitySource::TcpPeer,
                trusted_proxy_hops: 0,
            });
        }

        let forwarded_chain = has_forwarded
            .then(|| parse_forwarded(headers))
            .transpose()?;
        let x_forwarded_for_chain = has_x_forwarded_for
            .then(|| parse_x_forwarded_for(headers))
            .transpose()?;

        let (chain, source, header_kind) = match (forwarded_chain, x_forwarded_for_chain) {
            (Some(forwarded), Some(x_forwarded_for)) => {
                if forwarded != x_forwarded_for {
                    return Err(ClientIdentityError::with_hops(
                        ClientIdentityErrorReason::ConflictingForwardingHeaders,
                        ForwardingHeaderKind::Both,
                        forwarded.len().max(x_forwarded_for.len()),
                    ));
                }
                (
                    forwarded,
                    ClientIdentitySource::ConsistentForwardedPair,
                    ForwardingHeaderKind::Both,
                )
            }
            (Some(forwarded), None) => (
                forwarded,
                ClientIdentitySource::Forwarded,
                ForwardingHeaderKind::Forwarded,
            ),
            (None, Some(x_forwarded_for)) => (
                x_forwarded_for,
                ClientIdentitySource::XForwardedFor,
                ForwardingHeaderKind::XForwardedFor,
            ),
            (None, None) => unreachable!("header presence was checked above"),
        };

        if chain.len() > self.max_forwarded_hops {
            return Err(ClientIdentityError::with_hops(
                ClientIdentityErrorReason::ForwardedChainTooLong,
                header_kind,
                chain.len(),
            ));
        }

        let mut client_ip = peer_ip;
        let mut trusted_proxy_hops = 0;
        for candidate in chain.iter().rev() {
            if !self.is_trusted(client_ip) {
                break;
            }
            client_ip = *candidate;
            trusted_proxy_hops += 1;
        }

        Ok(ClientIdentity {
            client_ip,
            peer_addr,
            source,
            trusted_proxy_hops,
        })
    }

    fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted_proxy_cidrs.iter().any(|network| {
            network.contains(&ip)
                || matches!(
                    ip,
                    IpAddr::V4(ipv4)
                        if network.contains(&IpAddr::V6(ipv4.to_ipv6_mapped()))
                )
        })
    }
}

pub async fn manager_client_identity_middleware(
    State(resolver): State<Arc<ClientIdentityResolver>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    match resolve_http_request(&resolver, &request, "manager") {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(HttpClientIdentityError::MissingConnectInfo) => {
            crate::controller::BaseError::InternalServerError(Some(
                "client identity unavailable".to_string(),
            ))
            .into_response()
        }
        Err(HttpClientIdentityError::InvalidForwardingMetadata) => {
            crate::controller::BaseError::ParamInvalid(Some(
                "invalid client forwarding metadata".to_string(),
            ))
            .into_response()
        }
    }
}

pub async fn proxy_client_identity_middleware(
    resolver: Arc<ClientIdentityResolver>,
    downstream_protocol: DownstreamProtocol,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    match resolve_http_request(&resolver, &request, "proxy") {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(HttpClientIdentityError::MissingConnectInfo) => proxy_client_identity_error_response(
            &request,
            downstream_protocol,
            ProxyError::gateway(
                ProxyErrorCode::ServerError,
                ExecutionStage::Receive,
                ResponseVisibility::NotVisible,
                None,
                "client identity unavailable",
            ),
        ),
        Err(HttpClientIdentityError::InvalidForwardingMetadata) => {
            proxy_client_identity_error_response(
                &request,
                downstream_protocol,
                ProxyError::gateway(
                    ProxyErrorCode::InvalidRequestError,
                    ExecutionStage::Receive,
                    ResponseVisibility::NotVisible,
                    Some("invalid client forwarding metadata".to_string()),
                    "invalid client forwarding metadata",
                ),
            )
        }
    }
}

fn proxy_client_identity_error_response(
    request: &Request<Body>,
    downstream_protocol: DownstreamProtocol,
    error: ProxyError,
) -> Response {
    let request_context = request
        .extensions()
        .get::<Arc<ProxyRequestContext>>()
        .expect("proxy client identity must run after request identity middleware");
    ProtocolErrorResponseAdapter::new(downstream_protocol, request_context.request_id.clone())
        .proxy_error_response(error)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpClientIdentityError {
    MissingConnectInfo,
    InvalidForwardingMetadata,
}

fn resolve_http_request(
    resolver: &ClientIdentityResolver,
    request: &Request<Body>,
    surface: &'static str,
) -> Result<ClientIdentity, HttpClientIdentityError> {
    let Some(ConnectInfo(peer_addr)) = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied()
    else {
        crate::error_event!(
            "ingress.client_identity_missing_connect_info",
            surface = surface,
            reason = "missing_connect_info",
        );
        return Err(HttpClientIdentityError::MissingConnectInfo);
    };

    resolver
        .resolve(peer_addr, request.headers())
        .map_err(|error| {
            let peer_addr = normalize_socket_addr(peer_addr).to_string();
            let forwarded_header_count = request.headers().get_all(FORWARDED).iter().count();
            let x_forwarded_for_header_count =
                request.headers().get_all(X_FORWARDED_FOR).iter().count();
            crate::warn_event!(
                "ingress.client_identity_rejected",
                surface = surface,
                reason = error.reason.as_str(),
                peer_addr = &peer_addr,
                header_kind = error.header_kind.as_str(),
                observed_hops = error.observed_hops,
                forwarded_header_count = forwarded_header_count,
                x_forwarded_for_header_count = x_forwarded_for_header_count,
            );
            HttpClientIdentityError::InvalidForwardingMetadata
        })
}

fn parse_forwarded(headers: &HeaderMap) -> Result<Vec<IpAddr>, ClientIdentityError> {
    let mut chain = Vec::new();
    for value in headers.get_all(FORWARDED) {
        let value = value.to_str().map_err(|_| {
            ClientIdentityError::new(
                ClientIdentityErrorReason::InvalidHeaderEncoding,
                ForwardingHeaderKind::Forwarded,
            )
        })?;
        validate_forwarded_for_cardinality(value)?;
        for parsed in rfc7239::parse(value) {
            let forwarded = parsed.map_err(|_| {
                ClientIdentityError::new(
                    ClientIdentityErrorReason::InvalidForwardedSyntax,
                    ForwardingHeaderKind::Forwarded,
                )
            })?;
            let node = forwarded.forwarded_for.ok_or_else(|| {
                ClientIdentityError::new(
                    ClientIdentityErrorReason::ForwardedMissingFor,
                    ForwardingHeaderKind::Forwarded,
                )
            })?;
            let ip = node.ip().copied().ok_or_else(|| {
                ClientIdentityError::new(
                    ClientIdentityErrorReason::ForwardedNonIpFor,
                    ForwardingHeaderKind::Forwarded,
                )
            })?;
            chain.push(normalize_ip(ip));
        }
    }
    Ok(chain)
}

fn validate_forwarded_for_cardinality(value: &str) -> Result<(), ClientIdentityError> {
    let mut element_start = 0;
    let mut parameter_start = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    let mut for_count = 0;

    for (index, character) in value.char_indices() {
        if escaped {
            // rfc7239 0.1.3 does not preserve quoted-pair semantics, so fail closed.
            return Err(ClientIdentityError::new(
                ClientIdentityErrorReason::InvalidForwardedSyntax,
                ForwardingHeaderKind::Forwarded,
            ));
        }
        match character {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                count_forwarded_for_parameter(&value[parameter_start..index], &mut for_count)?;
                parameter_start = index + character.len_utf8();
            }
            ',' if !in_quotes => {
                count_forwarded_for_parameter(&value[parameter_start..index], &mut for_count)?;
                validate_forwarded_element(value, element_start, index, for_count)?;
                element_start = index + character.len_utf8();
                parameter_start = element_start;
                for_count = 0;
            }
            _ => {}
        }
    }

    if in_quotes || escaped {
        return Err(ClientIdentityError::new(
            ClientIdentityErrorReason::InvalidForwardedSyntax,
            ForwardingHeaderKind::Forwarded,
        ));
    }
    count_forwarded_for_parameter(&value[parameter_start..], &mut for_count)?;
    validate_forwarded_element(value, element_start, value.len(), for_count)
}

fn count_forwarded_for_parameter(
    parameter: &str,
    for_count: &mut usize,
) -> Result<(), ClientIdentityError> {
    let (name, _) = parameter.split_once('=').ok_or_else(|| {
        ClientIdentityError::new(
            ClientIdentityErrorReason::InvalidForwardedSyntax,
            ForwardingHeaderKind::Forwarded,
        )
    })?;
    if name.trim().eq_ignore_ascii_case("for") {
        *for_count += 1;
        if *for_count > 1 {
            return Err(ClientIdentityError::new(
                ClientIdentityErrorReason::ForwardedDuplicateFor,
                ForwardingHeaderKind::Forwarded,
            ));
        }
    }
    Ok(())
}

fn validate_forwarded_element(
    value: &str,
    start: usize,
    end: usize,
    for_count: usize,
) -> Result<(), ClientIdentityError> {
    if value[start..end].trim().is_empty() {
        return Err(ClientIdentityError::new(
            ClientIdentityErrorReason::InvalidForwardedSyntax,
            ForwardingHeaderKind::Forwarded,
        ));
    }
    if for_count != 1 {
        return Err(ClientIdentityError::new(
            ClientIdentityErrorReason::ForwardedMissingFor,
            ForwardingHeaderKind::Forwarded,
        ));
    }
    Ok(())
}

fn parse_x_forwarded_for(headers: &HeaderMap) -> Result<Vec<IpAddr>, ClientIdentityError> {
    let mut chain = Vec::new();
    for value in headers.get_all(X_FORWARDED_FOR) {
        let value = value.to_str().map_err(|_| {
            ClientIdentityError::new(
                ClientIdentityErrorReason::InvalidHeaderEncoding,
                ForwardingHeaderKind::XForwardedFor,
            )
        })?;
        for node in value.split(',') {
            let node = node.trim();
            if node.is_empty() {
                return Err(ClientIdentityError::new(
                    ClientIdentityErrorReason::InvalidXForwardedFor,
                    ForwardingHeaderKind::XForwardedFor,
                ));
            }
            let ip = parse_x_forwarded_for_node(node).ok_or_else(|| {
                ClientIdentityError::new(
                    ClientIdentityErrorReason::InvalidXForwardedFor,
                    ForwardingHeaderKind::XForwardedFor,
                )
            })?;
            chain.push(normalize_ip(ip));
        }
    }
    Ok(chain)
}

fn parse_x_forwarded_for_node(node: &str) -> Option<IpAddr> {
    if let Ok(ip) = node.parse::<IpAddr>() {
        return Some(ip);
    }
    if node.starts_with('[') && node.ends_with(']') {
        return node[1..node.len() - 1].parse::<IpAddr>().ok();
    }
    node.parse::<SocketAddr>().ok().map(|address| address.ip())
}

fn normalize_socket_addr(address: SocketAddr) -> SocketAddr {
    SocketAddr::new(normalize_ip(address.ip()), address.port())
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ipv6) => ipv6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ipv6)),
        IpAddr::V4(_) => ip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn resolver(cidrs: &[&str], max_forwarded_hops: usize) -> ClientIdentityResolver {
        ClientIdentityResolver::new(&ClientIdentityConfig {
            trusted_proxy_cidrs: cidrs
                .iter()
                .map(|cidr| cidr.parse().expect("test CIDR should parse"))
                .collect(),
            max_forwarded_hops,
        })
    }

    fn peer(ip: &str) -> SocketAddr {
        SocketAddr::new(ip.parse().expect("test peer IP should parse"), 4242)
    }

    #[test]
    fn client_identity_resolver_ignores_all_forwarding_headers_from_untrusted_peer() {
        let resolver = resolver(&[], 8);
        let mut headers = HeaderMap::new();
        headers.insert(FORWARDED, HeaderValue::from_static("not-valid"));
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("also-not-valid"));
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.10"));

        let identity = resolver
            .resolve(peer("192.0.2.7"), &headers)
            .expect("untrusted forwarding metadata must be ignored");
        assert_eq!(identity.client_ip, "192.0.2.7".parse::<IpAddr>().unwrap());
        assert_eq!(identity.source, ClientIdentitySource::TcpPeer);
        assert_eq!(identity.trusted_proxy_hops, 0);
    }

    #[test]
    fn client_identity_resolver_trusted_peer_without_supported_header_uses_tcp_peer() {
        let resolver = resolver(&["10.0.0.0/8"], 8);
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.10"));

        let identity = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect("x-real-ip must be ignored");
        assert_eq!(identity.client_ip, "10.0.0.9".parse::<IpAddr>().unwrap());
        assert_eq!(identity.source, ClientIdentitySource::TcpPeer);
    }

    #[test]
    fn client_identity_resolver_forwarded_supports_quoted_ipv6_ports_and_multiple_lines() {
        let resolver = resolver(&["10.0.0.0/8", "2001:db8:ffff::/48"], 8);
        let mut headers = HeaderMap::new();
        headers.append(
            FORWARDED,
            HeaderValue::from_static("for=\"[2001:db8::10]:443\";proto=https"),
        );
        headers.append(
            FORWARDED,
            HeaderValue::from_static("for=\"[2001:db8:ffff::1]:8443\""),
        );

        let identity = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect("valid Forwarded chain should resolve");
        assert_eq!(
            identity.client_ip,
            "2001:db8::10".parse::<IpAddr>().unwrap()
        );
        assert_eq!(identity.source, ClientIdentitySource::Forwarded);
        assert_eq!(identity.trusted_proxy_hops, 2);
    }

    #[test]
    fn client_identity_resolver_xff_supports_ipv4_ipv6_ports_and_multiple_lines() {
        let resolver = resolver(&["10.0.0.0/8", "192.0.2.0/24"], 8);
        let mut headers = HeaderMap::new();
        headers.append(
            X_FORWARDED_FOR,
            HeaderValue::from_static("[2001:db8::20]:443, 192.0.2.10:8080"),
        );
        headers.append(X_FORWARDED_FOR, HeaderValue::from_static("10.0.0.8"));

        let identity = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect("valid X-Forwarded-For chain should resolve");
        assert_eq!(
            identity.client_ip,
            "2001:db8::20".parse::<IpAddr>().unwrap()
        );
        assert_eq!(identity.source, ClientIdentitySource::XForwardedFor);
        assert_eq!(identity.trusted_proxy_hops, 3);
    }

    #[test]
    fn client_identity_resolver_stops_at_first_untrusted_node_from_the_right() {
        let resolver = resolver(&["10.0.0.0/8"], 8);
        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR,
            HeaderValue::from_static("198.51.100.4, 203.0.113.5, 10.0.0.8"),
        );

        let identity = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect("mixed chain should resolve");
        assert_eq!(identity.client_ip, "203.0.113.5".parse::<IpAddr>().unwrap());
        assert_eq!(identity.trusted_proxy_hops, 2);
    }

    #[test]
    fn client_identity_resolver_requires_complete_dual_header_equality() {
        let resolver = resolver(&["10.0.0.0/8"], 8);
        let mut headers = HeaderMap::new();
        headers.insert(
            FORWARDED,
            HeaderValue::from_static("for=203.0.113.1, for=10.0.0.8"),
        );
        headers.insert(
            X_FORWARDED_FOR,
            HeaderValue::from_static("203.0.113.1, 10.0.0.8"),
        );

        let identity = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect("matching forwarding headers should resolve");
        assert_eq!(
            identity.source,
            ClientIdentitySource::ConsistentForwardedPair
        );

        headers.insert(
            X_FORWARDED_FOR,
            HeaderValue::from_static("203.0.113.1, 10.0.0.7"),
        );
        let error = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect_err("different forwarding chains must be rejected");
        assert_eq!(
            error.reason,
            ClientIdentityErrorReason::ConflictingForwardingHeaders
        );
        assert_eq!(error.header_kind, ForwardingHeaderKind::Both);
    }

    #[test]
    fn client_identity_resolver_rejects_invalid_forwarded_nodes_and_cardinality() {
        let resolver = resolver(&["10.0.0.0/8"], 8);
        let cases = [
            ("for=unknown", ClientIdentityErrorReason::ForwardedNonIpFor),
            ("for=_hidden", ClientIdentityErrorReason::ForwardedNonIpFor),
            (
                "proto=https",
                ClientIdentityErrorReason::ForwardedMissingFor,
            ),
            (
                "for=192.0.2.1;for=192.0.2.2",
                ClientIdentityErrorReason::ForwardedDuplicateFor,
            ),
            (
                "for=192.0.2.1,,for=192.0.2.2",
                ClientIdentityErrorReason::InvalidForwardedSyntax,
            ),
            (
                "for=\"[2001:db8::1]:invalid\"",
                ClientIdentityErrorReason::InvalidForwardedSyntax,
            ),
            (
                "for=\"[2001:db8::1]",
                ClientIdentityErrorReason::InvalidForwardedSyntax,
            ),
        ];

        for (value, expected_reason) in cases {
            let mut headers = HeaderMap::new();
            headers.insert(
                FORWARDED,
                HeaderValue::from_str(value).expect("test header should construct"),
            );
            let error = resolver
                .resolve(peer("10.0.0.9"), &headers)
                .expect_err("invalid Forwarded value must be rejected");
            assert_eq!(
                error.reason, expected_reason,
                "unexpected reason for test value"
            );
        }
    }

    #[test]
    fn client_identity_resolver_rejects_invalid_xff_and_overlong_chains() {
        let resolver = resolver(&["10.0.0.0/8"], 2);
        for value in ["192.0.2.1,,10.0.0.8", "192.0.2.1:invalid", "unknown"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                X_FORWARDED_FOR,
                HeaderValue::from_str(value).expect("test header should construct"),
            );
            let error = resolver
                .resolve(peer("10.0.0.9"), &headers)
                .expect_err("invalid X-Forwarded-For value must be rejected");
            assert_eq!(
                error.reason,
                ClientIdentityErrorReason::InvalidXForwardedFor
            );
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR,
            HeaderValue::from_static("192.0.2.1, 10.0.0.7, 10.0.0.8"),
        );
        let error = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect_err("overlong chain must be rejected");
        assert_eq!(
            error.reason,
            ClientIdentityErrorReason::ForwardedChainTooLong
        );
        assert_eq!(error.observed_hops, Some(3));
    }

    #[test]
    fn client_identity_resolver_normalizes_ipv4_mapped_ipv6_before_matching() {
        let resolver = resolver(&["192.0.2.0/24"], 8);
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("198.51.100.8"));
        let identity = resolver
            .resolve(peer("::ffff:192.0.2.10"), &headers)
            .expect("mapped IPv4 peer should match IPv4 CIDR");

        assert_eq!(
            identity.peer_addr.ip(),
            "192.0.2.10".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            identity.client_ip,
            "198.51.100.8".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn client_identity_resolver_errors_do_not_expose_header_values() {
        let resolver = resolver(&["10.0.0.0/8"], 8);
        let secret_marker = "203.0.113.77";
        let mut headers = HeaderMap::new();
        headers.insert(
            FORWARDED,
            HeaderValue::from_str(&format!("for={secret_marker};for=192.0.2.1"))
                .expect("test header should construct"),
        );
        let error = resolver
            .resolve(peer("10.0.0.9"), &headers)
            .expect_err("duplicate Forwarded for must fail");

        assert!(!format!("{error}").contains(secret_marker));
        assert!(!format!("{error:?}").contains(secret_marker));
        assert_eq!(error.reason.as_str(), "duplicate_for");
    }
}
