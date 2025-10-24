import { useState } from "react";
import { createActor } from "./backend/api/hello_world";
import { getCanisterEnv } from "@icp-sdk/core/agent/canister-env";
import "./App.css";

// Here we define the environment variables that the asset canister serves.
// By default, the CLI sets all the canister IDs in the environment variables of the asset canister
// using the `PUBLIC_CANISTER_ID:<canister-name>` format.
// For this reason, we can expect the `PUBLIC_CANISTER_ID:backend` environment variable to be set.
interface CanisterEnv {
  readonly "PUBLIC_CANISTER_ID:backend": string;
}

// We only want to access the environment variables when serving the frontend from the asset canister.
// In development mode, we use a fixed canister ID for the backend canister.
const canisterEnv = getCanisterEnv<CanisterEnv>();
const canisterId = canisterEnv["PUBLIC_CANISTER_ID:backend"];

// We want to fetch the root key from the replica when developing locally.
const helloWorldActor = createActor(canisterId, {
  agentOptions: {
    rootKey: !import.meta.env.DEV ? canisterEnv!.IC_ROOT_KEY : undefined,
    shouldFetchRootKey: import.meta.env.DEV,
  },
});

function App() {
  const [greeting, setGreeting] = useState("");

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nameInput = (event.target as HTMLFormElement).elements.namedItem(
      "name"
    ) as HTMLInputElement;

    helloWorldActor.greet(nameInput.value).then(setGreeting);
    return false;
  }

  return (
    <main className="page">
      <section className="panel">
        <div className="brand" aria-label="ICP plus Vite">
          <img src="/icp.svg" alt="ICP logo" className="brand-icp" />
          <span className="plus">+</span>
          <img src="/vite.svg" alt="Vite logo" className="brand-vite" />
        </div>
        <h1 className="title">Frontend Environment Variables</h1>
        <p className="subtitle">
          Call the backend canister and get a greeting.
        </p>
        <form className="form" action="#" onSubmit={handleSubmit}>
          <label htmlFor="name">Enter your name</label>
          <div className="controls">
            <input
              id="name"
              alt="Name"
              type="text"
              className="input"
              placeholder="Ada Lovelace"
            />
            <button type="submit" className="button">
              Greet me
            </button>
          </div>
        </form>
        <section id="greeting" className="greeting" aria-live="polite">
          {greeting}
        </section>
      </section>
    </main>
  );
}

export default App;
