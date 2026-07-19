// Test file for Codasaurus review — v2
// Contains multiple intentional issues across different detectors

import { z } from "zod"; // phantom dep: zod used but not in package.json
import { createMagic } from "fakelib-nope"; // hallucinated import
import lodash from "lodash"; // phantom dep: lodash used but not declared

const STRIPE_KEY = "sk_live_51H7d8fJkLmNpQrStUvWxYzAbCdEfGhIjKlMnOp"; // hardcoded API key

// TODO: add rate limiting
async function fetchUser(id: string) {
  // FIXME: no error handling
  const res = await fetch(`https://api.example.com/users/${id}`);
  return res.json();
}

function ConfigFactory<T>(type: string): T {
  // Over-engineered: factory pattern for 1 variant
  if (type === "default") {
    return {} as T;
  }
  return {} as T;
}

export { fetchUser, ConfigFactory };
