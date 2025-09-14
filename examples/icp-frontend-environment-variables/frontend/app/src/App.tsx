import { useState } from "react";
import { createActor } from "./backend/api";
import { getCanisterEnv } from "@icp-sdk/canister-env";

const canisterEnv = getCanisterEnv();
const canisterId = canisterEnv["ICP_CANISTER_ID:backend"];

const helloWorldActor = createActor(canisterId, {
  agentOptions: { rootKey: canisterEnv.IC_ROOT_KEY },
});

function App() {
  const [greeting, setGreeting] = useState("");

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nameInput = (event.target as HTMLFormElement).elements.namedItem(
      "name"
    ) as HTMLInputElement;

    helloWorldActor.greet(nameInput.value).then((greeting) => {
      setGreeting(greeting);
    });
    return false;
  }

  return (
    <main>
      <form action="#" onSubmit={handleSubmit}>
        <label htmlFor="name">Enter your name: &nbsp;</label>
        <input id="name" alt="Name" type="text" />
        <button type="submit">Click Me!</button>
      </form>
      <section id="greeting">{greeting}</section>
    </main>
  );
}

export default App;
