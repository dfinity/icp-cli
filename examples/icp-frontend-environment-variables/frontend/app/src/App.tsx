import { useState } from "react";
import { createActor } from "./backend/api/hello_world";
import { getCanisterEnv } from "@icp-sdk/core/agent/canister-env";
import "./App.css";

// The ic_env cookie provides canister IDs and root key.
// Set by the asset canister (production) or dev server (development).
// See: https://github.com/dfinity/icp-cli/blob/main/docs/concepts/canister-discovery.md
interface CanisterEnv {
  readonly "PUBLIC_CANISTER_ID:backend": string;
  readonly IC_ROOT_KEY: Uint8Array; // Parsed from hex by the library
}

const canisterEnv = getCanisterEnv<CanisterEnv>();

const helloWorldActor = createActor(canisterEnv["PUBLIC_CANISTER_ID:backend"], {
  agentOptions: {
    rootKey: canisterEnv.IC_ROOT_KEY,
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
