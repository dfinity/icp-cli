import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import rehypeExternalLinks from 'rehype-external-links';
import agentDocs from './plugins/astro-agent-docs.mjs';

// https://astro.build/config
export default defineConfig({
  site: process.env.PUBLIC_SITE,
  // For versioned deployments: /icp-cli/0.1/, /icp-cli/0.2/, etc.
  // For non-versioned: /icp-cli/ in production, / in development
  // Defaults are set in the workflow, not here
  base: process.env.PUBLIC_BASE_PATH || (process.env.NODE_ENV === 'production' ? process.env.PUBLIC_BASE_PREFIX + '/' : '/'),
  markdown: {
    rehypePlugins: [
      // Open external links in new tab
      [rehypeExternalLinks, { target: '_blank', rel: ['noopener', 'noreferrer'] }],
    ],
  },
  integrations: [
    starlight({
      title: 'ICP CLI',
      description: 'Command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP)',
      favicon: '/favicon.png',
      components: {
        SiteTitle: './src/components/SiteTitle.astro',
      },
      head: [
        {
          // Agent-friendly docs: surface llms.txt directive early in <head>
          // so crawlers find it before the content area (agentdocsspec.com)
          tag: 'link',
          attrs: {
            rel: 'help',
            href: `${process.env.PUBLIC_BASE_PATH || '/'}llms.txt`,
            type: 'text/plain',
            title: 'LLM-friendly documentation index',
          },
        },
        {
          tag: 'script',
          attrs: {},
          content: `
            // Open social links in new tab
            document.addEventListener('DOMContentLoaded', () => {
              document.querySelectorAll('.social-icons a[href^="http"]').forEach(link => {
                link.setAttribute('target', '_blank');
                link.setAttribute('rel', 'noopener noreferrer');
              });
            });
          `,
        },
        // Matomo analytics — loaded from root so the site ID is defined once,
        // not baked into each versioned build
        {
          tag: 'script',
          attrs: { src: '/matomo.js', async: true },
        },
      ],
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/dfinity/icp-cli' },
      ],
      logo: {
        src: './src/assets/icp-logo.svg',
        replacesTitle: false,
        alt: 'ICP',
      },
      customCss: [
        './src/styles/layers.css',
        './src/styles/theme.css',
        './src/styles/overrides.css',
        './src/styles/elements.css',
      ],
      sidebar: [
        {
          label: 'Start Here',
          items: [
            { label: 'Quickstart', slug: 'quickstart' },
            { label: 'Tutorial', slug: 'tutorial' },
          ],
        },
        {
          label: 'Guides',
          items: [
            { label: 'Installation', slug: 'guides/installation' },
            { label: 'Local Development', slug: 'guides/local-development' },
            { label: 'Deploying to Mainnet', slug: 'guides/deploying-to-mainnet' },
            { label: 'Deploying to Specific Subnets', slug: 'guides/deploying-to-specific-subnets' },
            { label: 'Managing Environments', slug: 'guides/managing-environments' },
            { label: 'Managing Identities', slug: 'guides/managing-identities' },
            { label: 'Tokens and Cycles', slug: 'guides/tokens-and-cycles' },
            { label: 'Containerized Networks', slug: 'guides/containerized-networks' },
            { label: 'Using Recipes', slug: 'guides/using-recipes' },
            { label: 'Creating Recipes', slug: 'guides/creating-recipes' },
            { label: 'Creating Templates', slug: 'guides/creating-templates' },
          ],
        },
        {
          label: 'Concepts',
          items: [
            { label: 'Project Model', slug: 'concepts/project-model' },
            { label: 'Build, Deploy, Sync', slug: 'concepts/build-deploy-sync' },
            { label: 'Environments and Networks', slug: 'concepts/environments' },
            { label: 'Recipes', slug: 'concepts/recipes' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'CLI Reference', slug: 'reference/cli' },
            { label: 'Configuration Reference', slug: 'reference/configuration' },
            { label: 'Canister Settings', slug: 'reference/canister-settings' },
            { label: 'Environment Variables', slug: 'reference/environment-variables' },
          ],
        },
        {
          label: 'Migration',
          items: [
            { label: 'From dfx', slug: 'migration/from-dfx' },
          ],
        },
      ],
    }),
    // Generate .md endpoints, llms.txt, and agent signaling for agent-friendly docs.
    // Listed after starlight() so the astro:build:done hook runs after sitemap generation.
    agentDocs(),
  ],
});
