import {
  Actor,
  HttpAgent,
  type Agent,
  type HttpAgentOptions,
  type ActorConfig,
} from "@icp-sdk/core/agent";
import { type _SERVICE, idlFactory } from "./declarations/hello_world.did";
import { Hello_world, type ProcessErrorFn } from "./hello_world";

export interface CreateActorOptions {
  /**
   * @see {@link Agent}
   */
  agent?: Agent;
  /**
   * @see {@link HttpAgentOptions}
   */
  agentOptions?: HttpAgentOptions;
  /**
   * @see {@link ActorConfig}
   */
  actorOptions?: ActorConfig;
}

export function createActor(
  canisterId: string,
  options: CreateActorOptions = {},
  processError?: ProcessErrorFn
): Hello_world {
  const agent =
    options.agent || HttpAgent.createSync({ ...options.agentOptions });

  if (options.agent && options.agentOptions) {
    console.warn(
      "Detected both agent and agentOptions passed to createActor. Ignoring agentOptions and proceeding with the provided agent."
    );
  }

  // Creates an actor with using the candid interface and the HttpAgent
  const actor = Actor.createActor<_SERVICE>(idlFactory, {
    agent,
    canisterId,
    ...options.actorOptions,
  });

  return new Hello_world(actor, processError);
}
