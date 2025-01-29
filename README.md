# aika
An attempt at a lightweight Discrete Event Simulator (DES) for stable Rust! 

Prewarning: This is still an experiment at this stage so please don't expect much. There is also likely to be some broken stuff in here since the architecture is still morphing a ton based on the optimizations I figure out. 

### What and Why?

If you want to build a simulation that involves a shared world state and a consistent global clock, this library serves as that environment. Anything time-series, anything multi-agent and asynchronous, anything that requires event processing, whether in real-time or one-shot execution, this should be able to handle. (eventually...)

The idea behind this was to be able to provide all the flexibility of SimPy but with the extreme low level optimizations Rust can provide.

### Planned Features

- [x] Playback features for real-time simulations such as: pause, play, speed up and slow down
- [x] Hierarchical Timing Wheel with flexible hieght to maintain O(1) submit and retrieval times for near future events
- [x] Toggleable state logging for the world and agent states
- [ ] Methods for editing the event queue, callable to agents
- [ ] Messenger layer for agents to asynchronously communicate
- [ ] Test the universes
