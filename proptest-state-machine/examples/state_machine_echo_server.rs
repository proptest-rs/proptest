//-
// Copyright 2023 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! In this example, we're using the state machine testing to test interactions
//! of arbitrary client with an echo server, implemented using `message-io`
//! crate in the `system_under_test` module.

#[macro_use]
extern crate proptest_state_machine;

use std::collections::{HashMap, HashSet};
use std::thread;

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};

use system_under_test::{
    init_client, init_server, run_client, run_server, ClientDialer, Msg,
    ServerDialer, Transport,
};

// Setup the state machine test using the `prop_state_machine!` macro
prop_state_machine! {
    #![proptest_config(Config {
        // Turn failure persistence off for demonstration. This means that no
        // regression file will be captured.
        failure_persistence: None,
        // Enable verbose mode to make the state machine test print the
        // transitions for each case.
        verbose: 1,
        // Only run 10 cases by default to avoid running out of system resources
        // and taking too long to finish.
        cases: 10,
        .. Config::default()
    })]

    // NOTE: The `#[test]` attribute is commented out in here so we can run it
    // as an example from the `fn main`.

    // #[test]
    fn run_echo_server_test(
        // This is a macro's keyword - only `sequential` is currently supported.
        sequential
        // The number of transitions to be generated for each case. This can
        // be a single numerical value or a range as in here.
        1..20
        // Macro's boilerplate to separate the following identifier.
        =>
        // The name of the type that implements `StateMachineTest`.
        EchoServerTest
    );
}

fn main() {
    run_echo_server_test();
}

/// The reference state of the server and clients.
#[derive(Clone, Debug)]
struct RefState {
    /// The server status.
    is_server_up: bool,
    /// Set of client IDs that are connected.
    clients: HashSet<ClientId>,
    /// We randomly select which transport to use for the test case.
    transport: Transport,
}

/// The possible transitions of the state machine.
#[derive(Clone, Debug)]
enum Transition {
    StartServer,
    StopServer,
    StartClient(ClientId),
    StopClient(ClientId),
    ClientMsg(ClientId, Msg),
}

/// The state of the concrete server and clients under test.
#[derive(Default)]
struct EchoServerTest {
    server: Option<TestServer>,
    clients: HashMap<ClientId, TestClient>,
}

struct TestServer {
    /// A server dialer can be used to send message to clients and to shut-down
    /// the server.
    dialer: ServerDialer,
    /// The a handle of a thread that runs the server listener.
    listener_handle: thread::JoinHandle<()>,
}

struct TestClient {
    /// A client dialer can send messages to the server.
    dialer: ClientDialer,
    /// A handle of a thread that runs the client listener.
    listener_handle: std::thread::JoinHandle<()>,
    /// Messages received by the listener of the server are forwarded to this
    /// receiver, to be checked by the test.
    msgs_recv: std::sync::mpsc::Receiver<Msg>,
}

type ClientId = usize;

impl ReferenceStateMachine for RefState {
    type State = RefState;

    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Self::State> {
        prop_oneof![
            Just(Transport::Tcp),
            Just(Transport::FramedTcp),
            Just(Transport::Udp),
            Just(Transport::Ws),
        ]
        .prop_map(|transport| Self {
            is_server_up: false,
            clients: HashSet::default(),
            transport,
        })
        .boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        use Transition::*;
        if state.clients.is_empty() {
            prop_oneof![
                Just(StartServer),
                Just(StopServer),
                (0..32_usize).prop_map(StartClient),
            ]
            .boxed()
        } else {
            let ids: Vec<_> = state.clients.iter().cloned().collect();
            let arb_id = proptest::sample::select(ids);
            prop_oneof![
                Just(StartServer),
                Just(StopServer),
                (0..32_usize).prop_map(StartClient),
                arb_id.clone().prop_map(StopClient),
                arb_id.prop_flat_map(|id| arb_msg_from_client()
                    .prop_map(move |msg| { ClientMsg(id, msg) })),
            ]
            .boxed()
        }
    }

    fn apply(
        mut state: Self::State,
        transition: &Self::Transition,
    ) -> Self::State {
        match transition {
            Transition::StartServer => {
                state.is_server_up = true;
            }
            Transition::StopServer => {
                state.is_server_up = false;
                // Any existing clients will be disconnected.
                state.clients = Default::default();
            }
            Transition::StartClient(id) => {
                state.clients.insert(*id);
            }
            Transition::StopClient(id) => {
                state.clients.remove(id);
            }
            Transition::ClientMsg(_id, _msg) => {
                // Nothing to do in reference state.
            }
        }
        state
    }

    fn preconditions(
        state: &Self::State,
        transition: &Self::Transition,
    ) -> bool {
        match transition {
            Transition::StartServer => !state.is_server_up,
            Transition::StopServer => state.is_server_up,
            Transition::StartClient(id) => {
                // Only start clients if the server is running and this
                // client ID is not running already.
                state.is_server_up && !state.clients.contains(id)
            }
            Transition::StopClient(id) => {
                // Stop only if this client is actually running.
                state.clients.contains(id)
            }
            Transition::ClientMsg(id, _) => {
                // Can send only if both the server and this client are running.
                state.is_server_up && state.clients.contains(id)
            }
        }
    }
}

/// Generate an arbitrary MsgFromClient
fn arb_msg_from_client() -> impl Strategy<Value = Msg> {
    "[a-z0-9]{1,8}"
}

impl StateMachineTest for EchoServerTest {
    type SystemUnderTest = Self;

    type Reference = RefState;

    fn init_test(
        _ref_state: &<Self::Reference as ReferenceStateMachine>::State,
    ) -> Self::SystemUnderTest {
        Self::default()
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        ref_state: &<Self::Reference as ReferenceStateMachine>::State,
        transition: <Self::Reference as ReferenceStateMachine>::Transition,
    ) -> Self::SystemUnderTest {
        match transition {
            Transition::StartServer => {
                // Assign port dynamically
                let (dialer, listener) =
                    init_server(ref_state.transport, "127.0.0.1:0");

                // Run the listener in a new thread
                let listener_handle =
                    thread::spawn(move || run_server(listener));

                state.server = Some(TestServer {
                    dialer,
                    listener_handle,
                })
            }
            Transition::StopServer => {
                let server = state.server.take().unwrap();
                server.dialer.handler.stop();

                // Wait for the server listener to stop
                server.listener_handle.join().unwrap();

                if !state.clients.is_empty() {
                    println!(
                        "The server is waiting for all the clients to \
                             stop..."
                    );
                    for (id, client) in
                        std::mem::take(&mut state.clients).into_iter()
                    {
                        // Ask the client to stop
                        client.dialer.handler.stop();
                        println!("Asking client {} listener to stop.", id);
                        // Wait for it to actually stop
                        client.listener_handle.join().unwrap();
                        println!("Client {} listener stopped.", id);
                    }
                    println!("All clients have stopped.");
                }
            }
            Transition::StartClient(id) => {
                // Get the address of the server.
                let server_addr = state.server.as_ref().unwrap().dialer.address;

                let (listener, dialer) =
                    init_client(ref_state.transport, server_addr);

                // Open a channel for receiving message from the listener, so
                // that we can check the response the server.
                let (msgs_send, msgs_recv) = std::sync::mpsc::channel();

                let listener_handle = std::thread::spawn(move || {
                    run_client(listener, |msg| {
                        msgs_send.send(msg).unwrap();
                    })
                });

                state.clients.insert(
                    id,
                    TestClient {
                        dialer,
                        listener_handle,
                        msgs_recv,
                    },
                );
            }
            Transition::StopClient(id) => {
                // Remove the client
                let client = state.clients.remove(&id).unwrap();
                // Ask the client to stop
                client.dialer.handler.stop();
                // Wait for it to actually stop
                client.listener_handle.join().unwrap();
            }
            Transition::ClientMsg(id, msg) => {
                let client = state.clients.get_mut(&id).unwrap();

                // We use the broken implementation of msg_server, which should
                // be discovered by the test.
                system_under_test::msg_server_wrong(&mut client.dialer, &msg);

                // NOTE: To fix the issue that gets found by the state machine,
                // you can comment out the last statement with `pop_wrong` and
                // uncomment this one to see the test pass:
                // system_under_test::msg_server(&mut client.dialer, &msg);

                // Post-condition: The server must send a response back to the
                // client
                println!("Waiting for server response.");
                println!(
                    "WARN: Because we're using a blocking call here, this will \
                    halt when the message gets lost when `msg_server_wrong` is used."
                );
                let recv_msg = client.msgs_recv.recv().unwrap();
                assert_eq!(recv_msg, msg)
            }
        }
        state
    }
}

mod system_under_test {
    pub use message_io::network::Transport;
    use message_io::network::{Endpoint, NetEvent, ResourceId, ToRemoteAddr};
    use message_io::node::{self, NodeEvent, NodeHandler, NodeListener};

    use std::net::{SocketAddr, ToSocketAddrs};

    use std::sync::atomic::{self, AtomicBool};
    use std::sync::Arc;

    const ATOMIC_ORDER: atomic::Ordering = atomic::Ordering::SeqCst;

    /// We're only using valid UTF-8 strings here for messages to avoid having
    /// to pull another dev-dependency for serialization.
    pub type Msg = String;

    pub struct ServerListener {
        pub listener: NodeListener<()>,
        pub handler: NodeHandler<()>,
    }

    pub struct ServerDialer {
        pub address: SocketAddr,
        pub resource_id: ResourceId,
        pub handler: NodeHandler<()>,
    }

    pub struct ClientListener {
        pub address: SocketAddr,
        pub listener: NodeListener<()>,
        pub server: Endpoint,
        pub handler: NodeHandler<()>,
        /// Server connection status, shared with the [`ClientDialer`].
        pub is_connected: Arc<AtomicBool>,
    }

    pub struct ClientDialer {
        pub address: SocketAddr,
        pub server: Endpoint,
        pub handler: NodeHandler<()>,
        /// Server connection status, shared with the [`ClientListener`].
        pub is_connected: Arc<AtomicBool>,
    }

    pub fn init_server(
        transport: Transport,
        addr: impl ToSocketAddrs,
    ) -> (ServerDialer, ServerListener) {
        let (handler, listener) = node::split::<()>();

        let (resource_id, address) =
            handler.network().listen(transport, addr).unwrap();
        println!("Server is running at {address} with {transport}.");

        (
            ServerDialer {
                address,
                resource_id,
                handler: handler.clone(),
            },
            ServerListener { listener, handler },
        )
    }

    pub fn run_server(listener: ServerListener) {
        let ServerListener { listener, handler } = listener;

        listener.for_each(move |event| match event.network() {
            NetEvent::Connected(_, _) => (), // Only generated at connect() calls.
            NetEvent::Accepted(endpoint, _resource_id) => {
                // Only connection oriented protocols will generate this event
                println!("Client ({}) connected.", endpoint.addr(),);
            }
            NetEvent::Message(endpoint, msg_bytes) => {
                let message: Msg =
                    String::from_utf8(msg_bytes.to_vec()).unwrap();
                println!("Server received a message \"{message}\".");
                handler.network().send(endpoint, msg_bytes);
            }
            NetEvent::Disconnected(endpoint) => {
                // Only connection oriented protocols will generate this event
                println!("Client ({}) disconnected.", endpoint.addr());
            }
        });
    }

    pub fn init_client(
        transport: Transport,
        remote_addr: impl ToRemoteAddr,
    ) -> (ClientListener, ClientDialer) {
        let (handler, listener) = node::split();
        let (server, address) =
            handler.network().connect(transport, remote_addr).unwrap();

        let is_connected = Arc::new(AtomicBool::new(false));
        (
            ClientListener {
                address,
                server,
                handler: handler.clone(),
                listener,
                is_connected: is_connected.clone(),
            },
            ClientDialer {
                address,
                server,
                handler,
                is_connected,
            },
        )
    }

    pub fn run_client(listener: ClientListener, mut on_msg: impl FnMut(Msg)) {
        let ClientListener {
            address,
            server,
            handler,
            listener,
            is_connected,
        } = listener;

        listener.for_each(move |event| match event {
            NodeEvent::Network(net_event) => match net_event {
                NetEvent::Connected(_, established) => {
                    if established {
                        println!(
                            "Client identified by local port: {}.",
                            address.port()
                        );
                    } else {
                        println!("Cannot connect to server at {server}.")
                    }
                    is_connected.store(established, ATOMIC_ORDER);
                }
                NetEvent::Accepted(_, _) => unreachable!(), // Only generated when a listener accepts
                NetEvent::Message(_, msg_bytes) => {
                    let message: Msg =
                        String::from_utf8(msg_bytes.to_vec()).unwrap();
                    on_msg(message);
                }
                NetEvent::Disconnected(_) => {
                    println!("Server is disconnected.");
                    is_connected.store(false, ATOMIC_ORDER);
                    handler.stop();
                }
            },
            NodeEvent::Signal(()) => {
                // unused
            }
        });
    }

    /// This function will lose messages when they are sent before the client
    /// connection is established.
    pub fn msg_server_wrong(dialer: &mut ClientDialer, msg: &Msg) {
        let output_data = msg.as_bytes();

        dialer.handler.network().send(dialer.server, output_data);
    }

    #[allow(dead_code)]
    pub fn msg_server(dialer: &mut ClientDialer, msg: &Msg) {
        let output_data = msg.as_bytes();

        while !dialer.is_connected.load(ATOMIC_ORDER) {
            println!("Waiting for the server to be ready.");
        }

        dialer.handler.network().send(dialer.server, &output_data);
    }
}
