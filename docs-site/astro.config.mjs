import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import rehypeExternalLinks from 'rehype-external-links';

// https://astro.build/config
export default defineConfig({
  site: 'https://dfinity.github.io',
  base: process.env.NODE_ENV === 'production' ? '/icp-cli/' : '/',
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
      head: [
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
      ],
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/dfinity/icp-cli' },
      ],
      logo: {
        src: './src/assets/icp-logo.svg',
        replacesTitle: false,
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
  ],
});
