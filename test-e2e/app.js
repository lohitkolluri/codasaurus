// ====================================================================
// Codasaurus E2E Test File
// Exercises all detectors: hallucinated-imports, phantom-deps, secrets,
// todo-leaks, over-engineering, boilerplate, stale-api, vulnerabilities
// ====================================================================

// -------------------------------------------------------------------
// 1. HALLUCINATED IMPORTS (packages that don't exist on npm)
// -------------------------------------------------------------------
import { parse } from 'non-existent-package-xyz-12345';
const { format } = require('another-fake-package-67890');
import { render } from '@scope/completely-made-up-pkg';

// -------------------------------------------------------------------
// 2. PHANTOM DEPS (real packages not declared in package.json)
// -------------------------------------------------------------------
const _ = require('lodash');
const express = require('express');
const chalk = require('chalk');

// -------------------------------------------------------------------
// 3. SECRETS (credentials and tokens)
// -------------------------------------------------------------------
const AWS_ACCESS_KEY = 'AKIAIOSFODNN7EXAMPLE';
const GITHUB_PAT = 'ghp_1234567890abcdefghijklmnopqrstuvwxyz12';
const DB_CONNECTION = 'postgresql://admin:password123@prod-db.example.com:5432/mydb';
const JWT_SECRET = 'my-super-secret-key-that-should-not-be-here';
const PRIVATE_KEY = `-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA0gD0XHR4nF0Y6pIv9JzF0L0x0R0XHR4nF0Y6pIv9JzF0L0x
-----END RSA PRIVATE KEY-----`;

// -------------------------------------------------------------------
// 4. TODO / FIXME / HACK / XXX - leftover markers
// -------------------------------------------------------------------
// TODO: implement input validation
function process(data) {
    // FIXME: this query is vulnerable to SQL injection
    return `SELECT * FROM users WHERE id = ${data}`;
    // XXX: remove this before shipping
    // HACK: timeout workaround for race condition
}

// -------------------------------------------------------------------
// 5. OVER-ENGINEERING - unnecessary abstraction
// -------------------------------------------------------------------
class LoggerFactory {
    createLogger(type) {
        switch (type) {
            case 'console':
                return console;
            case 'file':
                return { log: (msg) => {} };
            default:
                return console;
        }
    }
}

// -------------------------------------------------------------------
// 6. BOILERPLATE - repeated long blocks
// -------------------------------------------------------------------
function validateUser(user) {
    if (!user.name) throw new Error('name required');
    if (!user.email) throw new Error('email required');
    if (!user.age) throw new Error('age required');
    if (!user.role) throw new Error('role required');
    if (!user.phone) throw new Error('phone required');
    if (!user.address) throw new Error('address required');
    if (!user.city) throw new Error('city required');
    if (!user.state) throw new Error('state required');
    if (!user.zip) throw new Error('zip required');
    if (!user.country) throw new Error('country required');
}

// Repeated block - nearly identical structure
function validateProduct(product) {
    if (!product.name) throw new Error('name required');
    if (!product.sku) throw new Error('sku required');
    if (!product.price) throw new Error('price required');
    if (!product.stock) throw new Error('stock required');
    if (!product.category) throw new Error('category required');
    if (!product.brand) throw new Error('brand required');
    if (!product.weight) throw new Error('weight required');
    if (!product.dimensions) throw new Error('dimensions required');
    if (!product.color) throw new Error('color required');
    if (!product.material) throw new Error('material required');
}

// -------------------------------------------------------------------
// 7. STALE API - deprecated patterns
// -------------------------------------------------------------------
var oldStyleVariable = 'deprecated';  // var instead of const/let
function legacyCallback(err, result) {
    if (err) throw err;  // callback pattern instead of async/await
    return result;
}
const legacyPromise = new Promise((resolve, reject) => {});  // unnecessary Promise wrapper

// -------------------------------------------------------------------
// 8. VULNERABILITIES - using known vulnerable packages
// - lodash@4.17.20 has CVE-2020-8203 (Prototype Pollution)
// - express@4.17.1 has CVE-2022-24999 (qs prototype pollution)
// -------------------------------------------------------------------
const result = _.defaults({}, { a: 1 });
const app = express();
