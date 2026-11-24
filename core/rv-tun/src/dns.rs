use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};

use crate::{DnsHop, DnsRoute, Handler, Packets, packet};

pub fn qname(msg: &[u8]) -> Option<String> {
    let mut i = 12;
    let mut name = String::new();
    loop {
        let len = *msg.get(i)? as usize;
        if len == 0 {
            break;
        }
        if len & 0xc0 != 0 || name.len() + len > 253 {
            return None;
        }
        let label = msg.get(i + 1..i + 1 + len)?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(label).to_ascii_lowercase());
        i += 1 + len;
    }
    (!name.is_empty()).then_some(name)
}

const IDLE: Duration = Duration::from_secs(90);
const FAIL_TTL: Duration = Duration::from_secs(3);
const MERGE_MAX: usize = 32;
const WORKER_QUEUE: usize = 256;
const DIRECT_MAX: usize = 128;
const RESOLVE_MAX: usize = 512;
const QUERY_TIMEOUT: Duration = Duration::from_secs(8);
const TCP_MSG_MAX: usize = 4096;
const TCP_PENDING_MAX: usize = 16;
const TCP_IDLE: Duration = Duration::from_secs(5);
const TCP_PENDING_IDLE: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HOLD: Duration = Duration::from_secs(2);
const HOLD_MAX: Duration = Duration::from_secs(30);

struct Query {
    msg: Vec<u8>,
    reply: oneshot::Sender<Vec<u8>>,
    retried: bool,
    limit: Duration,
}

#[derive(Default)]
pub(crate) struct Cache {
    map: HashMap<Vec<u8>, (Vec<u8>, tokio::time::Instant, tokio::time::Instant)>,
}

const CACHE_MAX: usize = 1024;
const CACHE_REPLY_MAX: usize = 1232;
const CACHE_TTL_MAX: u32 = 600;
const CACHE_NEG_MAX: u32 = 30;

impl Cache {
    fn get(&self, query: &[u8]) -> Option<Vec<u8>> {
        let (reply, at, until) = self.map.get(&cache_key(query)?)?;
        let now = tokio::time::Instant::now();
        if now >= *until {
            return None;
        }
        let mut r = for_query(reply.clone(), query)?;
        age_ttls(&mut r, (now - *at).as_secs() as u32);
        Some(r)
    }

    fn put(&mut self, query: &[u8], reply: &[u8]) -> bool {
        if !answers(query, reply) {
            return false;
        }
        let Some(key) = cache_key(query) else { return true };
        let ttl = match reply_ttl(reply) {
            Some(ttl) if ttl > 0 => Duration::from_secs(ttl as u64),
            None if reply[3] & 0x0f == 2 => FAIL_TTL,
            _ => return true,
        };
        if reply.len() > CACHE_REPLY_MAX {
            return true;
        }
        let now = tokio::time::Instant::now();
        if self.map.len() >= CACHE_MAX {
            self.map.retain(|_, (_, _, until)| *until > now);
            if self.map.len() >= CACHE_MAX {
                self.map.clear();
            }
        }
        self.map.insert(key, (reply.to_vec(), now, now + ttl));
        true
    }
}

fn for_query(mut r: Vec<u8>, query: &[u8]) -> Option<Vec<u8>> {
    r.get_mut(..2)?.copy_from_slice(query.get(..2)?);
    let end = skip_name(query, 12)?;
    r.get_mut(12..end)?.copy_from_slice(query.get(12..end)?);
    Some(r)
}

fn cache_key(msg: &[u8]) -> Option<Vec<u8>> {
    if msg.len() < 12 || u16::from_be_bytes([msg[4], msg[5]]) != 1 {
        return None;
    }
    let name_end = skip_name(msg, 12)?;
    let mut key = msg.get(12..name_end + 4)?.to_vec();
    key[..name_end - 12].make_ascii_lowercase();
    Some(key)
}

fn answers(query: &[u8], reply: &[u8]) -> bool {
    reply.len() >= 12
        && query.len() >= 12
        && reply[..2] == query[..2]
        && reply[2] & 0x80 != 0
        && cache_key(query).is_some_and(|q| cache_key(reply) == Some(q))
}

fn age_ttls(msg: &mut [u8], age: u32) {
    if age == 0 || msg.len() < 12 {
        return;
    }
    let count = |m: &[u8], i: usize| u16::from_be_bytes([m[i], m[i + 1]]) as usize;
    let (qd, an, ns) = (count(msg, 4), count(msg, 6), count(msg, 8));
    let mut i = 12;
    for _ in 0..qd {
        let Some(e) = skip_name(msg, i) else { return };
        i = e + 4;
    }
    for _ in 0..an + ns {
        let Some(e) = skip_name(msg, i) else { return };
        let Some(head) = msg.get(e..e + 10) else { return };
        let rtype = u16::from_be_bytes([head[0], head[1]]);
        let rdlen = u16::from_be_bytes([head[8], head[9]]) as usize;
        if rtype != 41 {
            let ttl = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
            msg[e + 4..e + 8].copy_from_slice(&ttl.saturating_sub(age).max(1).to_be_bytes());
        }
        i = e + 10 + rdlen;
    }
}

fn reply_ttl(msg: &[u8]) -> Option<u32> {
    if msg.len() < 12 || msg[2] & 0x02 != 0 {
        return None;
    }
    let rcode = msg[3] & 0x0f;
    if rcode != 0 && rcode != 3 {
        return None;
    }
    let count = |i: usize| u16::from_be_bytes([msg[i], msg[i + 1]]) as usize;
    let (qd, an, ns) = (count(4), count(6), count(8));
    let mut i = 12;
    for _ in 0..qd {
        i = skip_name(msg, i)? + 4;
    }
    let mut min: Option<u32> = None;
    for _ in 0..an + ns {
        i = skip_name(msg, i)?;
        let ttl = u32::from_be_bytes(msg.get(i + 4..i + 8)?.try_into().ok()?);
        let rdlen = u16::from_be_bytes([*msg.get(i + 8)?, *msg.get(i + 9)?]) as usize;
        i += 10 + rdlen;
        min = Some(min.map_or(ttl, |m| m.min(ttl)));
    }
    Some(if an == 0 || rcode == 3 { min.unwrap_or(CACHE_NEG_MAX).min(CACHE_NEG_MAX) } else { min?.min(CACHE_TTL_MAX) })
}

fn skip_name(msg: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let len = *msg.get(i)? as usize;
        if len == 0 {
            return Some(i + 1);
        }
        if len & 0xc0 == 0xc0 {
            msg.get(i + 1)?;
            return Some(i + 2);
        }
        i += 1 + len;
    }
}

#[derive(Clone)]
pub(crate) struct Client {
    inner: Rc<Inner>,
}

#[derive(Clone)]
struct Asker {
    client: SocketAddr,
    server: SocketAddr,
    tcp: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

struct Inner {
    cached_net: Cell<u64>,
    workers: RefCell<HashMap<usize, mpsc::Sender<Query>>>,
    engine_gen: Cell<u64>,
    direct_inflight: Cell<usize>,
    inflight: Cell<usize>,
    cache: RefCell<Cache>,
    waiting: RefCell<HashMap<Vec<u8>, Vec<Merged>>>,
    handler: Rc<dyn Handler>,
    network: watch::Receiver<u64>,
    packets: Rc<Packets>,
}

type Merged = (Asker, Vec<u8>);

struct InFlight {
    inner: Rc<Inner>,
    key: Option<Vec<u8>>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Some(k) = self.key.take() {
            self.inner.waiting.borrow_mut().remove(&k);
        }
    }
}

impl Client {
    pub fn new(handler: Rc<dyn Handler>, network: watch::Receiver<u64>, packets: Rc<Packets>) -> Self {
        let net = *network.borrow();
        Self {
            inner: Rc::new(Inner {
                cached_net: Cell::new(net),
                workers: Default::default(),
                engine_gen: Default::default(),
                direct_inflight: Default::default(),
                inflight: Default::default(),
                cache: Default::default(),
                waiting: Default::default(),
                handler,
                network,
                packets,
            }),
        }
    }

    pub fn query(&self, msg: &[u8], client: SocketAddr, server: SocketAddr) {
        self.ask(msg, Asker { client, server, tcp: None });
    }

    pub async fn serve_tcp<S: AsyncRead + AsyncWrite + Unpin>(&self, mut conn: S, client: SocketAddr, server: SocketAddr) {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (mut buf, mut chunk) = (Vec::new(), [0u8; 1500]);
        let (mut pending, mut eof) = (0usize, false);
        let mut last = tokio::time::Instant::now();
        loop {
            while pending < TCP_PENDING_MAX && buf.len() >= 2 {
                let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
                if !(12..=TCP_MSG_MAX).contains(&len) {
                    return;
                }
                if buf.len() < 2 + len {
                    break;
                }
                let msg: Vec<u8> = buf.drain(..2 + len).skip(2).collect();
                rv_stats::add(rv_stats::Slot::DnsQueries, 1);
                if self.ask(&msg, Asker { client, server, tcp: Some(tx.clone()) }) {
                    pending += 1;
                } else if let Some(fail) = servfail(&msg) {
                    let _ = tx.send(fail);
                    pending += 1;
                }
            }
            if eof && pending == 0 {
                return;
            }
            let deadline = last + if pending == 0 { TCP_IDLE } else { TCP_PENDING_IDLE };
            tokio::select! {
                biased;
                r = rx.recv() => {
                    let Some(reply) = r else { return };
                    pending = pending.saturating_sub(1);
                    last = tokio::time::Instant::now();
                    if reply.len() > u16::MAX as usize {
                        continue;
                    }
                    let mut out = Vec::with_capacity(reply.len() + 2);
                    out.extend_from_slice(&(reply.len() as u16).to_be_bytes());
                    out.extend_from_slice(&reply);
                    match tokio::time::timeout(TCP_PENDING_IDLE, conn.write_all(&out)).await {
                        Ok(Ok(())) => {}
                        _ => return,
                    }
                }
                r = conn.read(&mut chunk), if !eof && pending < TCP_PENDING_MAX && buf.len() < 2 + TCP_MSG_MAX => match r {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        last = tokio::time::Instant::now();
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    Err(_) => return,
                },
                _ = tokio::time::sleep_until(deadline) => return,
            }
        }
    }

    fn ask(&self, msg: &[u8], asker: Asker) -> bool {
        if msg.len() < 12 {
            return false;
        }
        let inner = &self.inner;
        let net = *inner.network.borrow();
        if inner.cached_net.replace(net) != net {
            inner.cache.borrow_mut().map.clear();
        }
        inner.check_engine();
        let cached = if inner.handler.dns_cache() { inner.cache.borrow().get(msg) } else { None };
        if let Some(reply) = cached {
            tracing::debug!(target: "rv::dns", "{} cached", qname(msg).unwrap_or_default());
            inner.reply(&asker, &reply);
            return true;
        }
        let key = cache_key(msg);
        if let Some((list, kept)) = key.as_ref().and_then(|k| {
            inner.waiting.borrow_mut().get_mut(k).map(|l| {
                let kept = l.len() < MERGE_MAX;
                if kept {
                    l.push((asker.clone(), msg.to_vec()));
                }
                (l.len(), kept)
            })
        }) {
            tracing::debug!(target: "rv::dns", "{} merged waiting={list}", qname(msg).unwrap_or_default());
            return kept;
        }
        let epoch = inner.epoch();
        let hops = match inner.handler.dns_route(msg) {
            DnsRoute::Reply(r) => {
                inner.reply(&asker, &r);
                return true;
            }
            DnsRoute::Servers(hops) => hops,
            DnsRoute::Lookup(name) => {
                if inner.inflight.get() >= RESOLVE_MAX {
                    return false;
                }
                inner.inflight.set(inner.inflight.get() + 1);
                let flight = inner.start(key);
                let msg = msg.to_vec();
                tokio::task::spawn_local(async move {
                    lookup(flight.inner.clone(), msg, name, asker, epoch).await;
                    drop(flight);
                });
                return true;
            }
        };
        if hops.is_empty() || inner.inflight.get() >= RESOLVE_MAX {
            return false;
        }
        inner.inflight.set(inner.inflight.get() + 1);
        let flight = inner.start(key);
        let msg = msg.to_vec();
        tokio::task::spawn_local(async move {
            resolve(flight.inner.clone(), msg, hops, asker, epoch).await;
            drop(flight);
        });
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Epoch {
    generation: u64,
    net: u64,
}

impl Inner {
    fn start(self: &Rc<Self>, key: Option<Vec<u8>>) -> InFlight {
        if let Some(k) = &key {
            self.waiting.borrow_mut().insert(k.clone(), Vec::new());
        }
        InFlight { inner: self.clone(), key }
    }

    fn answer(&self, asker: &Asker, query: &[u8], reply: &[u8]) {
        self.reply(asker, reply);
        let merged = cache_key(query).and_then(|k| self.waiting.borrow_mut().remove(&k)).unwrap_or_default();
        for (asker, q) in merged {
            if let Some(r) = for_query(reply.to_vec(), &q) {
                self.reply(&asker, &r);
            }
        }
    }

    fn reply(&self, asker: &Asker, reply: &[u8]) {
        if let Some(tx) = &asker.tcp {
            let _ = tx.send(reply.to_vec());
            return;
        }
        let (server, client) = (asker.server, asker.client);
        let mut out = Vec::with_capacity(reply.len() + 48);
        packet::build_udp(server, client, reply, &mut out);
        if !out.is_empty() {
            self.packets.push(out);
        }
    }

    fn check_engine(&self) {
        let generation = self.handler.generation();
        if self.engine_gen.replace(generation) != generation {
            self.workers.borrow_mut().clear();
            self.cache.borrow_mut().map.clear();
        }
    }

    fn epoch(&self) -> Epoch {
        Epoch { generation: self.handler.generation(), net: *self.network.borrow() }
    }

    fn remember(&self, epoch: Epoch, query: &[u8], reply: &[u8]) {
        if self.handler.dns_cache() && self.epoch() == epoch {
            self.cache.borrow_mut().put(query, reply);
        }
    }

    fn send(self: &Rc<Self>, id: usize, q: Query) -> bool {
        self.check_engine();
        let mut workers = self.workers.borrow_mut();
        let tx = workers.entry(id).or_insert_with(|| {
            let (tx, rx) = mpsc::channel(WORKER_QUEUE);
            tokio::task::spawn_local(worker(self.handler.clone(), id, rx, self.network.clone()));
            tx
        });
        tx.try_send(q).is_ok()
    }
}

const HEDGE: Duration = Duration::from_millis(800);

struct DirectSlot(Rc<Inner>);
impl Drop for DirectSlot {
    fn drop(&mut self) {
        self.0.direct_inflight.set(self.0.direct_inflight.get().saturating_sub(1));
    }
}

async fn attempt(inner: Rc<Inner>, hop: DnsHop, limit: Duration, msg: Vec<u8>) -> Option<Vec<u8>> {
    let t0 = Instant::now();
    let reply = match &hop {
        DnsHop::Direct(a) => {
            if inner.direct_inflight.get() >= DIRECT_MAX {
                return None;
            }
            inner.direct_inflight.set(inner.direct_inflight.get() + 1);
            let _slot = DirectSlot(inner.clone());
            let r = tokio::time::timeout(limit, inner.handler.dns_direct(*a, msg.clone())).await.ok().flatten();
            r.filter(|r| answers(&msg, r))
        }
        DnsHop::Stream(id) => {
            let (tx, rx) = oneshot::channel();
            if !inner.send(*id, Query { msg: msg.clone(), reply: tx, retried: false, limit }) {
                return None;
            }
            tokio::time::timeout(limit, rx).await.ok().and_then(Result::ok)
        }
    };
    let label = match &hop {
        DnsHop::Direct(a) => a.to_string(),
        DnsHop::Stream(id) => inner.handler.dns_label(*id),
    };
    tracing::debug!(
        target: "rv::dns",
        "{} via={label} ms={} rcode={} answers={} ttl={}",
        qname(&msg).unwrap_or_default(),
        t0.elapsed().as_millis(),
        reply.as_ref().map_or(-1, |r| i32::from(r[3] & 0x0f)),
        reply.as_ref().map_or(0, |r| u16::from_be_bytes([r[6], r[7]])),
        reply.as_deref().and_then(reply_ttl).map_or(-1, i64::from),
    );
    reply
}

#[derive(Clone)]
struct Planned {
    rank: usize,
    hop: DnsHop,
    limit: Duration,
    resend: bool,
}

struct Started {
    rank: usize,
    stream: bool,
    at: tokio::time::Instant,
    done: bool,
    task: tokio::task::JoinHandle<()>,
}

fn hedge_at(plan: &VecDeque<Planned>, started: &[Started]) -> Option<tokio::time::Instant> {
    let stream = matches!(plan.front()?.hop, DnsHop::Stream(_));
    let mut last = None;
    for s in started.iter().filter(|s| !s.done) {
        if s.stream != stream {
            return None;
        }
        last = last.max(Some(s.at));
    }
    last.map(|at| at + HEDGE)
}

async fn resolve(inner: Rc<Inner>, msg: Vec<u8>, hops: Vec<(DnsHop, Duration)>, asker: Asker, epoch: Epoch) {
    let n = hops.len();
    let t0 = Instant::now();
    let mut stale = false;
    let mut plan: VecDeque<Planned> =
        hops.into_iter().enumerate().map(|(rank, (hop, limit))| Planned { rank, hop, limit, resend: false }).collect();
    if n == 1 && matches!(plan[0].hop, DnsHop::Direct(_)) {
        let again = Planned { resend: true, ..plan[0].clone() };
        plan.push_back(again);
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<(usize, Option<Vec<u8>>)>();
    let mut started: Vec<Started> = Vec::new();
    let mut done = None;
    let mut fallback: Option<(usize, Vec<u8>)> = None;
    let mut fallback_at = None;
    loop {
        while plan.front().is_some_and(|p| p.resend && !started.iter().any(|s| !s.done && s.rank == p.rank)) {
            plan.pop_front();
        }
        let next = if started.iter().all(|s| s.done) {
            match plan.pop_front() {
                Some(p) => p,
                None => break,
            }
        } else {
            let hedge = hedge_at(&plan, &started);
            let give_up = fallback_at
                .filter(|_| plan.is_empty())
                .map(|at: tokio::time::Instant| at.max(started.iter().map(|s| s.at).max().unwrap_or(at)) + HEDGE);
            tokio::select! {
                biased;
                r = rx.recv() => {
                    let Some((i, reply)) = r else { break };
                    started[i].done = true;
                    let Some(reply) = reply else { continue };
                    if usable(&msg, &reply) {
                        done = Some(reply);
                        break;
                    }
                    let rank = started[i].rank;
                    if fallback.as_ref().is_none_or(|(r, _)| rank < *r) {
                        fallback = Some((rank, reply));
                    }
                    fallback_at.get_or_insert_with(tokio::time::Instant::now);
                    let higher = started.iter().any(|s| !s.done && s.rank < rank);
                    if plan.is_empty() && (stale || !higher) {
                        break;
                    }
                    continue;
                }
                _ = tokio::time::sleep_until(hedge.unwrap_or_else(tokio::time::Instant::now)), if hedge.is_some() => {
                    match plan.pop_front() {
                        Some(p) => p,
                        None => continue,
                    }
                }
                _ = tokio::time::sleep_until(give_up.unwrap_or_else(tokio::time::Instant::now)), if give_up.is_some() => break,
            }
        };
        if matches!(next.hop, DnsHop::Stream(_)) && inner.handler.generation() != epoch.generation {
            stale = true;
            plan.clear();
            continue;
        }
        let i = started.len();
        let Planned { rank, hop, limit, .. } = next;
        let stream = matches!(hop, DnsHop::Stream(_));
        let (inner, msg, tx) = (inner.clone(), msg.clone(), tx.clone());
        let task = tokio::task::spawn_local(async move {
            let _ = tx.send((i, attempt(inner, hop, limit, msg).await));
        });
        started.push(Started { rank, stream, at: tokio::time::Instant::now(), done: false, task });
    }
    for s in &started {
        s.task.abort();
    }
    let tried = started.len();
    let fallback = fallback.map(|(_, r)| r);
    if stale && done.is_none() {
        let epoch = inner.epoch();
        match inner.handler.dns_route(&msg) {
            DnsRoute::Servers(hops) if !hops.is_empty() => {
                return Box::pin(resolve(inner, msg, hops, asker, epoch)).await;
            }
            DnsRoute::Reply(r) => {
                inner.inflight.set(inner.inflight.get().saturating_sub(1));
                inner.answer(&asker, &msg, &r);
                return;
            }
            DnsRoute::Lookup(name) => {
                return lookup(inner, msg, name, asker, epoch).await;
            }
            DnsRoute::Servers(_) => {}
        }
    }
    inner.inflight.set(inner.inflight.get().saturating_sub(1));
    tracing::debug!(
        target: "rv::dns",
        "{} total_ms={} tried={tried}/{n} result={}",
        qname(&msg).unwrap_or_default(),
        t0.elapsed().as_millis(),
        if done.is_some() { "ok" } else if fallback.is_some() { "fallback" } else { "servfail" },
    );
    if let Some(reply) = done.or(fallback) {
        inner.remember(epoch, &msg, &reply);
        inner.answer(&asker, &msg, &reply);
    } else if let Some(fail) = servfail(&msg) {
        inner.remember(epoch, &msg, &fail);
        inner.answer(&asker, &msg, &fail);
    }
}

async fn lookup(inner: Rc<Inner>, msg: Vec<u8>, name: String, asker: Asker, epoch: Epoch) {
    let reply = inner.handler.dns_lookup(msg.clone(), name).await.or_else(|| servfail(&msg));
    inner.inflight.set(inner.inflight.get().saturating_sub(1));
    if let Some(reply) = reply {
        inner.remember(epoch, &msg, &reply);
        inner.answer(&asker, &msg, &reply);
    }
}

fn servfail(query: &[u8]) -> Option<Vec<u8>> {
    let end = skip_name(query, 12)? + 4;
    let mut r = query.get(..end)?.to_vec();
    r[2] = 0x80 | (r[2] & 0x79);
    r[3] = 0x80 | 2;
    r[4..6].copy_from_slice(&1u16.to_be_bytes());
    r[6..12].fill(0);
    Some(r)
}

fn usable(query: &[u8], reply: &[u8]) -> bool {
    if reply.len() < 12 {
        return false;
    }
    let rcode = reply[3] & 0x0f;
    let Some(qtype) = skip_name(query, 12).and_then(|e| query.get(e..e + 2)).map(|t| u16::from_be_bytes([t[0], t[1]])) else {
        return rcode == 0;
    };
    if qtype != 1 && qtype != 28 {
        return rcode != 2 && rcode != 5;
    }
    rcode == 0 && has_record(reply, qtype)
}

fn has_record(msg: &[u8], rtype: u16) -> bool {
    if msg.len() < 12 {
        return false;
    }
    let count = |i: usize| u16::from_be_bytes([msg[i], msg[i + 1]]) as usize;
    let (qd, an) = (count(4), count(6));
    let mut i = 12;
    for _ in 0..qd {
        let Some(e) = skip_name(msg, i) else { return false };
        i = e + 4;
    }
    for _ in 0..an {
        let Some(e) = skip_name(msg, i) else { return false };
        let Some(head) = msg.get(e..e + 10) else { return false };
        if u16::from_be_bytes([head[0], head[1]]) == rtype {
            return true;
        }
        i = e + 10 + u16::from_be_bytes([head[8], head[9]]) as usize;
    }
    false
}

struct Pending {
    query: Query,
    at: tokio::time::Instant,
}

fn stalls_at(p: &Pending, last_read: Option<tokio::time::Instant>) -> Option<tokio::time::Instant> {
    last_read.is_none_or(|r| r < p.at).then(|| p.at + p.query.limit.min(QUERY_TIMEOUT))
}

async fn worker(handler: Rc<dyn Handler>, upstream: usize, mut rx: mpsc::Receiver<Query>, mut network: watch::Receiver<u64>) {
    let name = handler.dns_label(upstream);
    let mut next_id: u16 = 1;
    let mut net_alive = true;
    let mut queue: Vec<Query> = Vec::new();
    let mut fails: u32 = 0;
    let mut held_until: Option<tokio::time::Instant> = None;
    loop {
        if queue.is_empty() {
            match rx.recv().await {
                Some(q) => queue.push(q),
                None => return,
            }
        }
        if network.has_changed().unwrap_or(false) {
            fails = 0;
            held_until = None;
        }
        if held_until.is_some_and(|t| tokio::time::Instant::now() < t) {
            queue.clear();
            while rx.try_recv().is_ok() {}
            continue;
        }
        network.mark_unchanged();
        let t0 = tokio::time::Instant::now();
        let stream = tokio::select! {
            r = tokio::time::timeout(CONNECT_TIMEOUT, handler.dns_stream(upstream)) => {
                r.unwrap_or_else(|_| Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timeout")))
            }
            r = network.changed(), if net_alive => {
                if r.is_err() {
                    net_alive = false;
                }
                fails = 0;
                held_until = None;
                while let Ok(q) = rx.try_recv() {
                    queue.push(q);
                }
                continue;
            }
        };
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                fails = fails.saturating_add(1);
                let hold = HOLD.saturating_mul(1 << (fails - 1).min(4)).min(HOLD_MAX);
                held_until = Some(tokio::time::Instant::now() + hold);
                if fails.is_power_of_two() {
                    tracing::warn!("dns upstream {name}: {e} (failures in a row: {fails}, held {}s)", hold.as_secs());
                } else {
                    tracing::debug!(target: "rv::dns", "dns upstream {name}: {e} (failures in a row: {fails})");
                }
                queue.clear();
                while rx.try_recv().is_ok() {}
                continue;
            }
        };
        fails = 0;
        held_until = None;
        tracing::debug!(target: "rv::dns", "upstream {name} connected ms={}", t0.elapsed().as_millis());
        let mut why = "broken";
        let mut pending: HashMap<u16, Pending> = HashMap::new();
        let mut last_active = tokio::time::Instant::now();
        let mut last_read: Option<tokio::time::Instant> = None;
        let mut rbuf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 4096];
        'conn: loop {
            let mut wbuf = Vec::new();
            for q in queue.drain(..) {
                if q.reply.is_closed() || pending.len() >= WORKER_QUEUE {
                    continue;
                }
                while pending.contains_key(&next_id) {
                    next_id = next_id.wrapping_add(1).max(1);
                }
                let id = next_id;
                next_id = next_id.wrapping_add(1).max(1);
                wbuf.extend_from_slice(&(q.msg.len() as u16).to_be_bytes());
                wbuf.extend_from_slice(&id.to_be_bytes());
                wbuf.extend_from_slice(&q.msg[2..]);
                pending.insert(id, Pending { query: q, at: tokio::time::Instant::now() });
            }
            if !wbuf.is_empty() {
                let sent = tokio::time::timeout(QUERY_TIMEOUT, async {
                    stream.write_all(&wbuf).await?;
                    stream.flush().await
                });
                let ok = tokio::select! {
                    r = sent => matches!(r, Ok(Ok(()))),
                    r = network.changed(), if net_alive => {
                        if r.is_err() {
                            net_alive = false;
                        }
                        why = "network";
                        false
                    }
                };
                if !ok {
                    break 'conn;
                }
                last_active = tokio::time::Instant::now();
            }
            let stall = pending.values().filter_map(|p| stalls_at(p, last_read)).min();
            let deadline = match pending.values().map(|p| p.at).min() {
                Some(oldest) => stall.map_or(oldest + QUERY_TIMEOUT, |s| s.min(oldest + QUERY_TIMEOUT)),
                None => last_active + IDLE,
            };
            tokio::select! {
                biased;
                r = stream.read(&mut tmp) => {
                    let n = match r { Ok(0) | Err(_) => break 'conn, Ok(n) => n };
                    last_active = tokio::time::Instant::now();
                    last_read = Some(last_active);
                    rbuf.extend_from_slice(&tmp[..n]);
                    while rbuf.len() >= 2 {
                        let len = u16::from_be_bytes([rbuf[0], rbuf[1]]) as usize;
                        if rbuf.len() < 2 + len { break; }
                        let mut msg: Vec<u8> = rbuf.drain(..2 + len).skip(2).collect();
                        if msg.len() < 12 { continue; }
                        let id = u16::from_be_bytes([msg[0], msg[1]]);
                        let ours = pending.get(&id).is_some_and(|p| {
                            let mut q = p.query.msg.clone();
                            q[..2].copy_from_slice(&msg[..2]);
                            answers(&q, &msg)
                        });
                        if let Some(p) = ours.then(|| pending.remove(&id)).flatten() {
                            msg[..2].copy_from_slice(&p.query.msg[..2]);
                            let _ = p.query.reply.send(msg);
                        }
                    }
                },
                q = rx.recv() => match q {
                    Some(q) => queue.push(q),
                    None => return,
                },
                r = network.changed(), if net_alive => {
                    if r.is_err() {
                        net_alive = false;
                        continue;
                    }
                    why = "network";
                    break 'conn;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    if pending.is_empty() {
                        why = "idle";
                        break 'conn;
                    }
                    let now = tokio::time::Instant::now();
                    if stall.is_some_and(|s| s <= now) {
                        why = "stalled";
                        break 'conn;
                    }
                    let before = pending.len();
                    pending.retain(|_, p| now - p.at < QUERY_TIMEOUT && !p.query.reply.is_closed());
                    tracing::debug!(target: "rv::dns", "timeout queries={}", before - pending.len());
                    if pending.is_empty() {
                        why = "timeout";
                        break 'conn;
                    }
                },
            }
        }
        tracing::debug!(target: "rv::dns", "upstream {name} closed reason={why} pending={}", pending.len());
        let now = tokio::time::Instant::now();
        queue.extend(
            pending
                .into_values()
                .filter(|p| !p.query.retried && now - p.at < QUERY_TIMEOUT && !p.query.reply.is_closed())
                .map(|p| Query { retried: true, ..p.query }),
        );
        while let Ok(q) = rx.try_recv() {
            queue.push(q);
        }
    }
}

#[cfg(kani)]
#[path = "proofs/dns.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/dns.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    struct Lossy {
        calls: Cell<u32>,
        dead: Option<SocketAddr>,
        lose_first: bool,
    }

    fn answer(q: &[u8]) -> Vec<u8> {
        let mut r = q.to_vec();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 1;
        r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]);
        r
    }

    impl crate::Handler for Lossy {
        fn tcp(&self, _: crate::TcpConn, _: SocketAddr, _: SocketAddr) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
        fn dns_stream(&self, _: usize) -> crate::LocalBoxFuture<std::io::Result<crate::BoxStream>> {
            Box::pin(async { Err(std::io::Error::other("none")) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            DnsRoute::Servers(Vec::new())
        }
        fn dns_direct(&self, server: SocketAddr, msg: Vec<u8>) -> crate::LocalBoxFuture<Option<Vec<u8>>> {
            let n = self.calls.get();
            self.calls.set(n + 1);
            let silent = Some(server) == self.dead || (self.lose_first && n == 0);
            Box::pin(async move {
                if silent {
                    std::future::pending::<()>().await;
                }
                Some(answer(&msg))
            })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            None
        }
        fn udp(&self, _: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: crate::UdpReply) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
    }

    async fn resolved_after(h: Lossy, hops: Vec<SocketAddr>) -> (Duration, u32) {
        let h = Rc::new(h);
        let (_net_tx, net) = watch::channel(0u64);
        let packets = Rc::new(Packets::default());
        let inner = Rc::new(Inner {
            cached_net: Default::default(),
            workers: Default::default(),
            engine_gen: Default::default(),
            direct_inflight: Default::default(),
            inflight: Cell::new(1),
            cache: Default::default(),
            waiting: Default::default(),
            handler: h.clone(),
            network: net,
            packets: packets.clone(),
        });
        let t0 = tokio::time::Instant::now();
        let hops = hops.into_iter().map(|a| (DnsHop::Direct(a), Duration::from_secs(4))).collect();
        let (c, s) = ("10.0.0.2:5353".parse().unwrap(), "172.19.0.2:53".parse().unwrap());
        resolve(inner.clone(), query("a.example"), hops, Asker { client: c, server: s, tcp: None }, inner.epoch()).await;
        assert!(!packets.empty(), "ответ ушёл в TUN");
        tokio::task::yield_now().await;
        assert_eq!(inner.direct_inflight.get(), 0, "отменённые попытки освободили места");
        (t0.elapsed(), h.calls.get())
    }

    #[tokio::test(start_paused = true)]
    async fn lost_query_to_single_server_is_resent_before_timeout() {
        let a = "192.0.2.1:53".parse().unwrap();
        let (t, calls) = tokio::task::LocalSet::new()
            .run_until(resolved_after(Lossy { calls: Cell::new(0), dead: None, lose_first: true }, vec![a]))
            .await;
        assert_eq!(calls, 2);
        assert!(t >= HEDGE && t < HEDGE + Duration::from_millis(100), "{t:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn silent_server_does_not_hold_the_next_one() {
        let (a, b) = ("192.0.2.1:53".parse().unwrap(), "192.0.2.2:53".parse().unwrap());
        let (t, calls) = tokio::task::LocalSet::new()
            .run_until(resolved_after(Lossy { calls: Cell::new(0), dead: Some(a), lose_first: false }, vec![a, b]))
            .await;
        assert_eq!(calls, 2);
        assert!(t < HEDGE + Duration::from_millis(100), "{t:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn fast_server_needs_one_query() {
        let (a, b) = ("192.0.2.1:53".parse().unwrap(), "192.0.2.2:53".parse().unwrap());
        let (t, calls) = tokio::task::LocalSet::new()
            .run_until(resolved_after(Lossy { calls: Cell::new(0), dead: None, lose_first: false }, vec![a, b]))
            .await;
        assert_eq!(calls, 1);
        assert!(t < Duration::from_millis(10), "{t:?}");
    }

    fn query(name: &str) -> Vec<u8> {
        let mut q = vec![0xab, 0xcd, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in name.split('.') {
            q.push(l.len() as u8);
            q.extend_from_slice(l.as_bytes());
        }
        q.extend_from_slice(&[0, 0, 1, 0, 1]);
        q
    }

    fn reply(q: &[u8], ttls: &[u32], rcode: u8) -> Vec<u8> {
        let mut r = q.to_vec();
        r[2] = 0x81;
        r[3] = 0x80 | rcode;
        r[7] = ttls.len() as u8;
        for t in ttls {
            r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
            r.extend_from_slice(&t.to_be_bytes());
            r.extend_from_slice(&[0, 4, 1, 2, 3, 4]);
        }
        r
    }

    #[test]
    fn fuzz_short_message_many_answers() {
        let mut m = vec![0x6a, 0x6a, 0, 0, 0, 0, 0xb8, 0, 0, 0];
        m.extend(std::iter::repeat_n(0x6a, 22));
        m.push(0x0a);
        let _ = qname(&m);
        let _ = answers(&m, &m);
        let _ = usable(&m, &m);
        let _ = skip_name(&m, 12);
        let mut c = m.clone();
        age_ttls(&mut c, 5);
        let _ = reply_ttl(&m);
        let _ = has_record(&m, 1);
    }

    #[test]
    fn servfail_echoes_question() {
        let mut q = query("a.test");
        q[2] = 0x01;
        q.extend_from_slice(&[0, 0, 41, 16, 0, 0, 0, 0, 0, 0, 0]);
        q[11] = 1;
        let r = servfail(&q).unwrap();
        assert_eq!(r.len(), query("a.test").len());
        assert_eq!((r[2], r[3] & 0x0f), (0x81, 2));
        assert_eq!(u16::from_be_bytes([r[4], r[5]]), 1);
        assert!(r[6..12].iter().all(|b| *b == 0));
        assert!(!usable(&query("a.test"), &r));
        assert!(servfail(&[0; 5]).is_none());
    }

    #[test]
    fn usable_replies() {
        let q = query("a.test");
        assert!(usable(&q, &reply(&q, &[60], 0)));
        assert!(!usable(&q, &reply(&q, &[], 0)));
        assert!(!usable(&q, &reply(&q, &[], 3)));
        assert!(!usable(&q, &reply(&q, &[], 2)));
        let mut h = query("a.test");
        let n = h.len();
        h[n - 3] = 65;
        assert!(usable(&h, &reply(&h, &[], 0)));
        assert!(usable(&h, &reply(&h, &[], 3)));
        assert!(!usable(&h, &reply(&h, &[], 5)));
    }

    #[test]
    fn ttl_rules() {
        let q = query("a.test");
        assert_eq!(reply_ttl(&reply(&q, &[300, 120], 0)), Some(120));
        assert_eq!(reply_ttl(&reply(&q, &[86400], 0)), Some(CACHE_TTL_MAX));
        assert_eq!(reply_ttl(&reply(&q, &[], 3)), Some(CACHE_NEG_MAX));
        assert_eq!(reply_ttl(&reply(&q, &[60], 2)), None);
    }

    #[test]
    fn cache_answers_with_callers_id() {
        let mut c = Cache::default();
        let q = query("Example.test");
        c.put(&q, &reply(&q, &[60], 0));
        let mut q2 = query("example.TEST");
        q2[0] = 0x77;
        let hit = c.get(&q2).unwrap();
        assert_eq!(&hit[..2], &[0x77, 0xcd]);
        assert!(c.get(&query("other.test")).is_none());
        let z = query("zero.test");
        c.put(&z, &reply(&z, &[0], 0));
        assert!(c.get(&z).is_none());
    }

    #[test]
    fn cache_echoes_the_question_as_asked() {
        let mut c = Cache::default();
        let q = query("ExAmPlE.test");
        c.put(&q, &reply(&q, &[60], 0));
        let q2 = query("eXaMpLe.TEST");
        let hit = c.get(&q2).unwrap();
        assert_eq!(&hit[12..q2.len()], &q2[12..]);
        assert_eq!(&hit[q2.len()..], &reply(&q, &[60], 0)[q.len()..]);
    }

    #[test]
    fn cache_rejects_foreign_replies_and_ages_ttl() {
        let mut c = Cache::default();
        let q = query("a.test");
        let mut wrong_id = reply(&q, &[60], 0);
        wrong_id[1] ^= 1;
        assert!(!c.put(&q, &wrong_id));
        assert!(!c.put(&q, &reply(&query("b.test"), &[60], 0)));
        assert!(c.get(&q).is_none());
        let mut t65 = query("a.test");
        let n = t65.len();
        t65[n - 3] = 65;
        let mut t97 = t65.clone();
        t97[n - 3] = 97;
        assert_ne!(cache_key(&t65), cache_key(&t97));
        let mut r = reply(&q, &[60], 0);
        age_ttls(&mut r, 45);
        assert_eq!(reply_ttl(&r), Some(15));
    }

    const APP: &str = "10.0.0.2:5353";

    struct Slow {
        calls: Cell<u32>,
        delay: Option<Duration>,
    }

    impl crate::Handler for Slow {
        fn tcp(&self, _: crate::TcpConn, _: SocketAddr, _: SocketAddr) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
        fn dns_stream(&self, _: usize) -> crate::LocalBoxFuture<std::io::Result<crate::BoxStream>> {
            Box::pin(async { Err(std::io::Error::other("none")) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            DnsRoute::Servers(vec![(DnsHop::Direct("192.0.2.1:53".parse().unwrap()), Duration::from_secs(4))])
        }
        fn dns_direct(&self, _: SocketAddr, msg: Vec<u8>) -> crate::LocalBoxFuture<Option<Vec<u8>>> {
            self.calls.set(self.calls.get() + 1);
            let delay = self.delay;
            Box::pin(async move {
                match delay {
                    Some(d) => tokio::time::sleep(d).await,
                    None => std::future::pending::<()>().await,
                }
                Some(answer(&msg))
            })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            None
        }
        fn udp(&self, _: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: crate::UdpReply) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
    }

    fn client(h: Rc<Slow>) -> (Client, Rc<Packets>, watch::Sender<u64>) {
        let (net_tx, net) = watch::channel(0u64);
        let packets = Rc::new(Packets::default());
        (Client::new(h, net, packets.clone()), packets, net_tx)
    }

    fn ids(p: &Packets) -> Vec<u16> {
        p.pending.borrow().iter().map(|b| u16::from_be_bytes([b[28], b[29]])).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn failed_lookup_is_remembered_briefly() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = Rc::new(Slow { calls: Cell::new(0), delay: None });
                let (c, packets, _net) = client(h.clone());
                let (app, peer) = (APP.parse().unwrap(), PEER.parse().unwrap());
                c.query(&query("dead.test"), app, peer);
                tokio::time::sleep(Duration::from_secs(6)).await;
                assert_eq!(rcodes(&packets), [2], "SERVFAIL after the server's time");
                let asked = h.calls.get();
                c.query(&query("dead.test"), app, peer);
                tokio::task::yield_now().await;
                assert_eq!(rcodes(&packets), [2, 2], "the second asker gets it at once");
                assert_eq!(h.calls.get(), asked, "from the cache");
                tokio::time::sleep(FAIL_TTL).await;
                c.query(&query("dead.test"), app, peer);
                tokio::task::yield_now().await;
                assert!(h.calls.get() > asked, "asked again once it expired");
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn identical_queries_share_one_lookup() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = Rc::new(Slow { calls: Cell::new(0), delay: Some(Duration::from_millis(300)) });
                let (c, packets, _net) = client(h.clone());
                let peer = PEER.parse().unwrap();
                let (mut q1, mut q2) = (query("same.test"), query("SAME.test"));
                q1[..2].copy_from_slice(&[0, 1]);
                q2[..2].copy_from_slice(&[0, 2]);
                c.query(&q1, APP.parse().unwrap(), peer);
                c.query(&q2, "10.0.0.3:5353".parse().unwrap(), peer);
                c.query(&query("other.test"), APP.parse().unwrap(), peer);
                tokio::time::sleep(Duration::from_secs(1)).await;
                assert_eq!(h.calls.get(), 2, "same.test once, other.test once");
                let mut got = ids(&packets);
                got.sort();
                assert_eq!(got, [1, 2, 0xabcd], "each asker gets its own id");
                let second = packets.pending.borrow().iter().find(|b| b[29] == 2).cloned().unwrap();
                assert_eq!(&second[28 + 12..28 + q2.len() - 4], &q2[12..q2.len() - 4], "and its own spelling of the name");
                assert!(c.inner.waiting.borrow().is_empty());
            })
            .await;
    }

    const PEER: &str = "172.19.0.2:53";

    fn udp_asker() -> Asker {
        Asker { client: APP.parse().unwrap(), server: PEER.parse().unwrap(), tcp: None }
    }

    fn rig(h: Rc<dyn crate::Handler>) -> (Rc<Inner>, Rc<Packets>, watch::Sender<u64>) {
        let (net_tx, net) = watch::channel(0u64);
        let packets = Rc::new(Packets::default());
        let inner = Rc::new(Inner {
            cached_net: Default::default(),
            workers: Default::default(),
            engine_gen: Default::default(),
            direct_inflight: Default::default(),
            inflight: Default::default(),
            cache: Default::default(),
            waiting: Default::default(),
            handler: h,
            network: net,
            packets: packets.clone(),
        });
        (inner, packets, net_tx)
    }

    fn rcodes(p: &Packets) -> Vec<u8> {
        p.pending.borrow().iter().map(|b| b[28 + 3] & 0x0f).collect()
    }

    async fn serve(mut s: tokio::io::DuplexStream) {
        loop {
            let mut len = [0u8; 2];
            if s.read_exact(&mut len).await.is_err() {
                return;
            }
            let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
            if s.read_exact(&mut q).await.is_err() {
                return;
            }
            let r = answer(&q);
            let mut out = (r.len() as u16).to_be_bytes().to_vec();
            out.extend_from_slice(&r);
            if s.write_all(&out).await.is_err() {
                return;
            }
        }
    }

    struct Streams {
        opened: Cell<u32>,
        dead: RefCell<Vec<tokio::io::DuplexStream>>,
    }

    impl crate::Handler for Streams {
        fn tcp(&self, _: crate::TcpConn, _: SocketAddr, _: SocketAddr) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
        fn dns_stream(&self, _: usize) -> crate::LocalBoxFuture<std::io::Result<crate::BoxStream>> {
            self.opened.set(self.opened.get() + 1);
            let (near, far) = tokio::io::duplex(64 * 1024);
            if self.opened.get() == 1 {
                self.dead.borrow_mut().push(far);
            } else {
                tokio::task::spawn_local(serve(far));
            }
            Box::pin(async move { Ok(Box::new(near) as crate::BoxStream) })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            DnsRoute::Servers(Vec::new())
        }
        fn dns_direct(&self, _: SocketAddr, _: Vec<u8>) -> crate::LocalBoxFuture<Option<Vec<u8>>> {
            Box::pin(async { None })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            None
        }
        fn udp(&self, _: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: crate::UdpReply) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
    }

    #[tokio::test(start_paused = true)]
    async fn silently_dead_stream_is_replaced_while_queries_keep_coming() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = Rc::new(Streams { opened: Cell::new(0), dead: Default::default() });
                let (inner, packets, _net) = rig(h.clone());
                for i in 0..10 {
                    let hops = vec![(DnsHop::Stream(0), Duration::from_secs(4))];
                    tokio::task::spawn_local(resolve(inner.clone(), query(&format!("n{i}.example")), hops, udp_asker(), inner.epoch()));
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                assert_eq!(h.opened.get(), 2);
                let mut rc = rcodes(&packets);
                rc.sort();
                assert_eq!(rc, [vec![0; 9], vec![2]].concat());
            })
            .await;
    }

    #[derive(Clone, Copy)]
    enum Says {
        Addr,
        Rcode(u8),
        Nothing,
        Silent,
    }

    struct Net {
        direct: Vec<(SocketAddr, u64, Says)>,
        asked: RefCell<Vec<SocketAddr>>,
        stream: Option<(u64, bool)>,
        opened: Cell<u32>,
        generation: Cell<u64>,
        cache: Cell<bool>,
        route: RefCell<Vec<(DnsHop, Duration)>>,
        routes: Cell<u32>,
    }

    fn net(direct: &[(&str, u64, Says)], stream: Option<(u64, bool)>) -> Rc<Net> {
        Rc::new(Net {
            direct: direct.iter().map(|&(a, ms, s)| (a.parse().unwrap(), ms, s)).collect(),
            asked: Default::default(),
            stream,
            opened: Cell::new(0),
            generation: Cell::new(0),
            cache: Cell::new(true),
            route: Default::default(),
            routes: Cell::new(0),
        })
    }

    impl crate::Handler for Net {
        fn tcp(&self, _: crate::TcpConn, _: SocketAddr, _: SocketAddr) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
        fn generation(&self) -> u64 {
            self.generation.get()
        }
        fn dns_cache(&self) -> bool {
            self.cache.get()
        }
        fn dns_stream(&self, _: usize) -> crate::LocalBoxFuture<std::io::Result<crate::BoxStream>> {
            self.opened.set(self.opened.get() + 1);
            let (ms, ok) = self.stream.expect("stream");
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                if !ok {
                    return Err(std::io::Error::other("refused"));
                }
                let (near, far) = tokio::io::duplex(64 * 1024);
                tokio::task::spawn_local(serve(far));
                Ok(Box::new(near) as crate::BoxStream)
            })
        }
        fn dns_label(&self, _: usize) -> String {
            String::new()
        }
        fn dns_route(&self, _: &[u8]) -> DnsRoute {
            self.routes.set(self.routes.get() + 1);
            DnsRoute::Servers(self.route.borrow().clone())
        }
        fn dns_direct(&self, server: SocketAddr, msg: Vec<u8>) -> crate::LocalBoxFuture<Option<Vec<u8>>> {
            self.asked.borrow_mut().push(server);
            let (_, ms, says) = *self.direct.iter().find(|d| d.0 == server).expect("server");
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                match says {
                    Says::Addr => Some(answer(&msg)),
                    Says::Rcode(c) => Some(reply(&msg, &[], c)),
                    Says::Nothing => None,
                    Says::Silent => std::future::pending().await,
                }
            })
        }
        fn udp_route(&self, _: SocketAddr, _: SocketAddr) -> Option<u64> {
            None
        }
        fn udp(&self, _: mpsc::Receiver<(SocketAddr, Vec<u8>)>, _: crate::UdpReply) -> crate::LocalBoxFuture<()> {
            Box::pin(async {})
        }
    }

    const A: &str = "192.0.2.1:53";
    const B: &str = "192.0.2.2:53";
    const C: &str = "192.0.2.3:53";

    fn direct(a: &str) -> (DnsHop, Duration) {
        (DnsHop::Direct(a.parse().unwrap()), Duration::from_secs(4))
    }

    fn stream() -> (DnsHop, Duration) {
        (DnsHop::Stream(0), Duration::from_secs(4))
    }

    async fn ask(h: Rc<Net>, hops: Vec<(DnsHop, Duration)>) -> (Duration, Vec<u8>) {
        let (inner, packets, _net) = rig(h);
        let t0 = tokio::time::Instant::now();
        let epoch = inner.epoch();
        resolve(inner, query("a.example"), hops, udp_asker(), epoch).await;
        (t0.elapsed(), rcodes(&packets))
    }

    fn near(t: Duration, ms: u64) -> bool {
        t >= Duration::from_millis(ms) && t < Duration::from_millis(ms + 50)
    }

    #[tokio::test(start_paused = true)]
    async fn negative_answer_waits_for_a_higher_priority_server() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (t, rc) = ask(net(&[(A, 1200, Says::Addr), (B, 50, Says::Rcode(3))], None), vec![direct(A), direct(B)]).await;
                assert!(near(t, 1200) && rc == [0], "{t:?} {rc:?}");
                let (t, rc) = ask(net(&[(A, 1200, Says::Rcode(5)), (B, 50, Says::Rcode(3))], None), vec![direct(A), direct(B)]).await;
                assert!(near(t, 1200) && rc == [5], "{t:?} {rc:?}");
                let (t, rc) = ask(net(&[(A, 0, Says::Silent), (B, 50, Says::Rcode(3))], None), vec![direct(A), direct(B)]).await;
                assert!(near(t, 800 + 50 + 800) && rc == [3], "{t:?} {rc:?}");
                let (t, rc) = ask(net(&[(A, 1700, Says::Addr), (B, 50, Says::Rcode(3))], None), vec![direct(A), direct(B)]).await;
                assert!(near(t, 1650) && rc == [3], "{t:?} {rc:?}");
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn servers_of_different_kinds_are_asked_in_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = net(&[(B, 10, Says::Addr)], Some((1500, true)));
                let (t, rc) = ask(h.clone(), vec![stream(), direct(B)]).await;
                assert!(near(t, 1500) && rc == [0], "{t:?} {rc:?}");
                assert!(h.asked.borrow().is_empty());
                let h = net(&[(B, 10, Says::Addr)], Some((0, false)));
                let (t, rc) = ask(h.clone(), vec![stream(), direct(B)]).await;
                assert!(near(t, 10) && rc == [0], "{t:?} {rc:?}");
                let h = net(&[(A, 0, Says::Silent)], Some((0, true)));
                let (t, rc) = ask(h.clone(), vec![direct(A), stream()]).await;
                assert!(near(t, 4000) && rc == [0], "{t:?} {rc:?}");
                assert_eq!(h.opened.get(), 1);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn failed_hedge_does_not_restart_the_clock() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = net(&[(A, 0, Says::Silent), (B, 0, Says::Nothing), (C, 0, Says::Addr)], None);
                let (t, rc) = ask(h.clone(), vec![direct(A), direct(B), direct(C)]).await;
                assert!(near(t, 800) && rc == [0], "{t:?} {rc:?}");
                assert_eq!(*h.asked.borrow(), [A, B, C].map(|a| a.parse::<SocketAddr>().unwrap()));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn dead_stream_is_not_redialed_for_every_query() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = net(&[(B, 10, Says::Addr)], Some((300, false)));
                let (inner, packets, _net) = rig(h.clone());
                let hops = vec![stream(), direct(B)];
                for i in 0..3 {
                    let epoch = inner.epoch();
                    let t0 = tokio::time::Instant::now();
                    resolve(inner.clone(), query(&format!("n{i}.example")), hops.clone(), udp_asker(), epoch).await;
                    assert!(if i == 0 { near(t0.elapsed(), 310) } else { near(t0.elapsed(), 10) }, "{:?}", t0.elapsed());
                }
                assert_eq!(h.opened.get(), 1);
                assert_eq!(rcodes(&packets), [0, 0, 0]);
                tokio::time::sleep(HOLD).await;
                resolve(inner.clone(), query("late.example"), hops, udp_asker(), inner.epoch()).await;
                assert_eq!(h.opened.get(), 2);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn single_server_is_asked_again_only_while_silent() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = net(&[(A, 30, Says::Rcode(3))], None);
                let (t, rc) = ask(h.clone(), vec![direct(A)]).await;
                assert!(near(t, 30) && rc == [3], "{t:?} {rc:?}");
                assert_eq!(h.asked.borrow().len(), 1);
                let h = net(&[(A, 0, Says::Nothing)], None);
                let (t, rc) = ask(h.clone(), vec![direct(A)]).await;
                assert!(near(t, 0) && rc == [2], "{t:?} {rc:?}");
                assert_eq!(h.asked.borrow().len(), 1);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn cache_keeps_answers_only_from_the_engine_and_network_of_the_route() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for case in 0..4 {
                    let h = net(&[(A, 100, Says::Addr)], None);
                    h.cache.set(case != 1);
                    let (inner, packets, net_tx) = rig(h.clone());
                    let epoch = inner.epoch();
                    let h2 = h.clone();
                    tokio::task::spawn_local(async move {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        match case {
                            2 => h2.generation.set(1),
                            3 => drop(net_tx.send(1)),
                            _ => {}
                        }
                    });
                    resolve(inner.clone(), query("a.example"), vec![direct(A)], udp_asker(), epoch).await;
                    assert_eq!(rcodes(&packets), [0], "приложение ответ получает всегда");
                    assert_eq!(inner.cache.borrow().map.len(), usize::from(case == 0), "случай {case}");
                }
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn engine_switch_right_after_routing_reroutes_the_query() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = net(&[], Some((0, true)));
                *h.route.borrow_mut() = vec![stream()];
                let (_net_tx, net_rx) = watch::channel(0u64);
                let packets = Rc::new(Packets::default());
                let client = Client::new(h.clone(), net_rx, packets.clone());
                client.query(&query("a.example"), APP.parse().unwrap(), PEER.parse().unwrap());
                h.generation.set(1);
                tokio::time::sleep(Duration::from_secs(1)).await;
                assert_eq!(h.routes.get(), 2, "маршрут посчитан заново уже новым движком");
                assert_eq!(rcodes(&packets), [0]);
            })
            .await;
    }

    fn framed(q: &[u8]) -> Vec<u8> {
        let mut m = (q.len() as u16).to_be_bytes().to_vec();
        m.extend_from_slice(q);
        m
    }

    async fn read_msg(s: &mut tokio::io::DuplexStream) -> Option<Vec<u8>> {
        let mut len = [0u8; 2];
        s.read_exact(&mut len).await.ok()?;
        let mut m = vec![0u8; u16::from_be_bytes(len) as usize];
        s.read_exact(&mut m).await.ok()?;
        Some(m)
    }

    fn tcp_rig(h: Rc<Slow>) -> (tokio::io::DuplexStream, tokio::task::JoinHandle<()>) {
        let (app, ours) = tokio::io::duplex(65536);
        let (c, _, net) = client(h);
        let task = tokio::task::spawn_local(async move {
            let _keep = net;
            c.serve_tcp(ours, APP.parse().unwrap(), PEER.parse().unwrap()).await;
        });
        (app, task)
    }

    fn quick() -> Rc<Slow> {
        Rc::new(Slow { calls: Cell::new(0), delay: Some(Duration::ZERO) })
    }

    #[tokio::test(start_paused = true)]
    async fn tcp_query_gets_a_framed_answer_from_the_same_path() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = quick();
                let (mut app, _task) = tcp_rig(h.clone());
                let q = query("a.example");
                app.write_all(&framed(&q)).await.unwrap();
                let r = read_msg(&mut app).await.unwrap();
                assert_eq!(r, answer(&q));
                assert_eq!(h.calls.get(), 1);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn several_tcp_queries_in_one_connection_are_all_answered() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, _task) = tcp_rig(quick());
                let qs: Vec<Vec<u8>> = (0..3).map(|i| query(&format!("n{i}.example"))).collect();
                app.write_all(&qs.iter().flat_map(|q| framed(q)).collect::<Vec<_>>()).await.unwrap();
                let mut got = Vec::new();
                for _ in 0..3 {
                    got.push(read_msg(&mut app).await.unwrap());
                }
                got.sort();
                let mut want: Vec<_> = qs.iter().map(|q| answer(q)).collect();
                want.sort();
                assert_eq!(got, want);
                let q = query("late.example");
                app.write_all(&framed(&q)).await.unwrap();
                assert_eq!(read_msg(&mut app).await.unwrap(), answer(&q));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn tcp_query_arriving_in_pieces_is_assembled() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, _task) = tcp_rig(quick());
                let q = query("a.example");
                let m = framed(&q);
                for piece in [&m[..1], &m[1..2], &m[2..7], &m[7..]] {
                    app.write_all(piece).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert_eq!(read_msg(&mut app).await.unwrap(), answer(&q));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn tcp_bad_sizes_close_the_connection() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for len in [0u16, 5, TCP_MSG_MAX as u16 + 1, u16::MAX] {
                    let h = quick();
                    let (mut app, task) = tcp_rig(h.clone());
                    app.write_all(&len.to_be_bytes()).await.unwrap();
                    app.write_all(&[0u8; 32]).await.unwrap();
                    assert!(read_msg(&mut app).await.is_none(), "length {len}");
                    task.await.unwrap();
                    assert_eq!(h.calls.get(), 0);
                }
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn tcp_cut_in_the_middle_of_a_message_ends_quietly() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = quick();
                let (mut app, task) = tcp_rig(h.clone());
                let q = query("a.example");
                app.write_all(&framed(&q)[..10]).await.unwrap();
                app.shutdown().await.unwrap();
                task.await.unwrap();
                assert!(read_msg(&mut app).await.is_none());
                assert_eq!(h.calls.get(), 0);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn tcp_half_close_still_gets_the_answers() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, task) = tcp_rig(quick());
                let q = query("a.example");
                app.write_all(&framed(&q)).await.unwrap();
                app.shutdown().await.unwrap();
                assert_eq!(read_msg(&mut app).await.unwrap(), answer(&q));
                task.await.unwrap();
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn idle_tcp_connection_is_closed() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut app, task) = tcp_rig(quick());
                let q = query("a.example");
                app.write_all(&framed(&q)).await.unwrap();
                read_msg(&mut app).await.unwrap();
                tokio::time::sleep(TCP_IDLE - Duration::from_secs(1)).await;
                assert!(!task.is_finished());
                tokio::time::sleep(Duration::from_secs(2)).await;
                assert!(task.is_finished());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn tcp_queries_at_once_are_capped() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let h = Rc::new(Slow { calls: Cell::new(0), delay: None });
                let (mut app, task) = tcp_rig(h.clone());
                for i in 0..TCP_PENDING_MAX + 8 {
                    app.write_all(&framed(&query(&format!("n{i}.example")))).await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                assert_eq!(h.calls.get() as usize, TCP_PENDING_MAX);
                tokio::time::sleep(TCP_PENDING_IDLE + Duration::from_secs(1)).await;
                assert!(task.is_finished());
            })
            .await;
    }
}
