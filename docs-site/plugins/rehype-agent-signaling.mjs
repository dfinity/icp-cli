/**
 * Rehype plugin that injects an agent signaling directive at the top of every page.
 * Part of the Agent-Friendly Documentation spec (https://agentdocsspec.com).
 *
 * Adds a visually-hidden blockquote pointing agents to /llms.txt.
 * Uses CSS clip-rect (not display:none) so it survives HTML-to-markdown conversion.
 */

export default function rehypeAgentSignaling() {
  return (tree) => {
    const blockquote = {
      type: "element",
      tagName: "blockquote",
      properties: { className: ["agent-signaling"] },
      children: [
        {
          type: "element",
          tagName: "p",
          properties: {},
          children: [
            {
              type: "text",
              value: "For AI agents: Documentation index at ",
            },
            {
              type: "element",
              tagName: "a",
              properties: { href: "/llms.txt" },
              children: [{ type: "text", value: "/llms.txt" }],
            },
          ],
        },
      ],
    };

    tree.children.unshift(blockquote);
  };
}
