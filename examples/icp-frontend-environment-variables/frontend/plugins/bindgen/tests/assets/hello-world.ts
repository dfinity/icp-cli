export const helloWorldAsset = {
  result: `import { IDL } from '@dfinity/candid';

export const idlService = IDL.Service({
  'greet' : IDL.Func([IDL.Text], [IDL.Text], ['query']),
});

export const idlInitArgs = [];

export const idlFactory = ({ IDL }) => {
  return IDL.Service({ 'greet' : IDL.Func([IDL.Text], [IDL.Text], ['query']) });
};

export const init = ({ IDL }) => { return []; };`,
  path: "./tests/assets/hello_world.did",
};
