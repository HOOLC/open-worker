export async function resolve(specifier, context, nextResolve) {
  if (specifier === "@larksuiteoapi/node-sdk") {
    const parentUrl = context.parentURL ?? "";
    if (!parentUrl.includes("mock-lark-sdk.mjs")) {
      return {
        shortCircuit: true,
        url: new URL("./mock-lark-sdk.mjs", import.meta.url).href,
      };
    }
  }

  return nextResolve(specifier, context);
}
