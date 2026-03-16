import { defineCollection } from 'astro:content';
import { docsSchema } from '@astrojs/starlight/schema';
import { glob } from 'astro/loaders';

export const collections = {
  docs: defineCollection({
    loader: glob({
      pattern: ['**/*.md', '**/*.mdx', '!schemas/**', '!**/README.md', '!VERSIONED_DOCS.md'],
      base: '../docs',
    }),
    schema: docsSchema(),
  }),
};
