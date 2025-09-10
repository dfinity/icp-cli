export const helloWorldAsset = {
  result: {
    declarations_js: `import { IDL } from '@icp-sdk/core/candid';

export const idlService = IDL.Service({
  'greet' : IDL.Func([IDL.Text], [IDL.Text], ['query']),
});

export const idlInitArgs = [];

export const idlFactory = ({ IDL }) => {
  return IDL.Service({ 'greet' : IDL.Func([IDL.Text], [IDL.Text], ['query']) });
};

export const init = ({ IDL }) => { return []; };`,
    declarations_ts: `import type { ActorMethod } from '@icp-sdk/core/agent';
import type { IDL } from '@icp-sdk/core/candid';
import type { Principal } from '@icp-sdk/core/principal';

export interface _SERVICE { 'greet' : ActorMethod<[string], string> }
export declare const idlService: IDL.ServiceClass;
export declare const idlInitArgs: IDL.Type[];
export declare const idlFactory: IDL.InterfaceFactory;
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[];`,
    interface_ts: `import { type HttpAgentOptions, type ActorConfig, type Agent } from "@icp-sdk/core/agent";
import type { Principal } from "@icp-sdk/core/principal";
import { _SERVICE } from "./declarations/hello_world.did.d.ts";
export interface Some<T> {
    __kind__: "Some";
    value: T;
}
export interface None {
    __kind__: "None";
}
export type Option<T> = Some<T> | None;
export interface CreateActorOptions {
    agent?: Agent;
    agentOptions?: HttpAgentOptions;
    actorOptions?: ActorConfig;
}
export declare const createActor: (options?: CreateActorOptions, processError?: (error: unknown) => never) => hello_worldInterface;
export declare const canisterId: string;
export interface hello_worldInterface {
    greet(name: string): Promise<string>;
}
`,
    service_ts: `import { type HttpAgentOptions, type ActorConfig, type Agent, type ActorSubclass } from "@icp-sdk/core/agent";
import type { Principal } from "@icp-sdk/core/principal";
import { _SERVICE } from "./declarations/hello_world.did.d.ts";
export interface Some<T> {
    __kind__: "Some";
    value: T;
}
export interface None {
    __kind__: "None";
}
export type Option<T> = Some<T> | None;
function some<T>(value: T): Some<T> {
    return {
        __kind__: "Some",
        value: value
    };
}
function none(): None {
    return {
        __kind__: "None"
    };
}
function isNone<T>(option: Option<T>): option is None {
    return option.__kind__ === "None";
}
function isSome<T>(option: Option<T>): option is Some<T> {
    return option.__kind__ === "Some";
}
function unwrap<T>(option: Option<T>): T {
    if (isNone(option)) {
        throw new Error("unwrap: none");
    }
    return option.value;
}
function candid_some<T>(value: T): [T] {
    return [
        value
    ];
}
function candid_none<T>(): [] {
    return [];
}
function record_opt_to_undefined<T>(arg: T | null): T | undefined {
    return arg == null ? undefined : arg;
}
export interface CreateActorOptions {
    agent?: Agent;
    agentOptions?: HttpAgentOptions;
    actorOptions?: ActorConfig;
}
export function createActor(options?: CreateActorOptions, processError?: (error: unknown) => never): hello_worldInterface {
    const actor = _createActor(canisterId, options);
    return new Hello_world(actor, processError);
}
export const canisterId = _canisterId;
export interface hello_worldInterface {
    greet(name: string): Promise<string>;
}
class Hello_world implements hello_worldInterface {
    private actor: ActorSubclass<_SERVICE>;
    constructor(actor?: ActorSubclass<_SERVICE>, private processError?: (error: unknown) => never){
        this.actor = actor ?? _hello_world;
    }
    async greet(arg0: string): Promise<string> {
        if (this.processError) {
            try {
                const result = await this.actor.greet(arg0);
                return result;
            } catch (e) {
                this.processError(e);
                throw new Error("unreachable");
            }
        } else {
            const result = await this.actor.greet(arg0);
            return result;
        }
    }
}
export const hello_world: hello_worldInterface = new Hello_world();
`,
  },
  path: "./tests/assets/hello_world.did",
};
