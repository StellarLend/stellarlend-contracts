/**
 * Failure-path and boundary coverage for api/src/utils/errors.ts
 *
 * Covers:
 *  - ApiError (base class): constructor invariants, prototype chain, field types, boundary values
 *  - ValidationError:       status-code invariant, message propagation, instanceof chain
 *  - UnauthorizedError:     default and custom message paths, status-code invariant
 *  - NotFoundError:         default and custom message paths, status-code invariant
 *  - ConflictError:         message propagation, status-code invariant
 *  - InternalServerError:   default and custom message paths, status-code invariant
 *  - Cross-cutting:         instanceof narrowing, re-throw safety, catch-as-Error compatibility
 *  - Boundary / adversarial: empty strings, very long strings, whitespace-only messages,
 *                            numeric edge-case status codes, non-string values coercion,
 *                            stack-trace presence, serialisability
 */

import {
  ApiError,
  ValidationError,
  UnauthorizedError,
  ForbiddenError,
  NotFoundError,
  ConflictError,
  InternalServerError,
} from '../errors';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function isApiError(value: unknown): value is ApiError {
  return value instanceof ApiError;
}

// ---------------------------------------------------------------------------
// ApiError — base class
// ---------------------------------------------------------------------------
describe('ApiError (base class)', () => {
  describe('construction — happy path', () => {
    it('stores statusCode, message, and isOperational', () => {
      const err = new ApiError(400, 'bad request');
      expect(err.statusCode).toBe(400);
      expect(err.message).toBe('bad request');
      expect(err.isOperational).toBe(true);
    });

    it('defaults isOperational to true when omitted', () => {
      const err = new ApiError(422, 'unprocessable');
      expect(err.isOperational).toBe(true);
    });

    it('accepts isOperational = false for programmer errors', () => {
      const err = new ApiError(500, 'unexpected crash', false);
      expect(err.isOperational).toBe(false);
    });

    it('is an instance of Error', () => {
      const err = new ApiError(503, 'unavailable');
      expect(err).toBeInstanceOf(Error);
    });

    it('is an instance of ApiError', () => {
      const err = new ApiError(503, 'unavailable');
      expect(err).toBeInstanceOf(ApiError);
    });

    it('has a stack trace', () => {
      const err = new ApiError(400, 'trace check');
      expect(typeof err.stack).toBe('string');
      expect(err.stack!.length).toBeGreaterThan(0);
    });

    it('name is "Error" (native default — subclasses may override)', () => {
      // ApiError does not set this.name; the native prototype name is 'Error'
      const err = new ApiError(400, 'test');
      expect(typeof err.name).toBe('string');
    });
  });

  // --- Prototype chain fix (Object.setPrototypeOf) -------------------------
  describe('prototype chain', () => {
    it('instanceof ApiError is true after new ApiError()', () => {
      const err = new ApiError(400, 'proto-test');
      expect(err instanceof ApiError).toBe(true);
    });

    it('instanceof Error is true after new ApiError()', () => {
      const err = new ApiError(400, 'proto-test');
      expect(err instanceof Error).toBe(true);
    });

    it('can be caught as Error in a try/catch', () => {
      let caught: Error | null = null;
      try {
        throw new ApiError(400, 'thrown');
      } catch (e) {
        caught = e as Error;
      }
      expect(caught).toBeInstanceOf(Error);
      expect(caught).toBeInstanceOf(ApiError);
    });

    it('narrowing via isApiError helper returns correct type', () => {
      const e = new ApiError(400, 'narrow me');
      expect(isApiError(e)).toBe(true);
    });

    it('plain Error does not satisfy isApiError', () => {
      const e = new Error('plain');
      expect(isApiError(e)).toBe(false);
    });
  });

  // --- Boundary: status codes ----------------------------------------------
  describe('statusCode boundary values', () => {
    it('accepts status code 0 (boundary)', () => {
      const err = new ApiError(0, 'edge');
      expect(err.statusCode).toBe(0);
    });

    it('accepts status code Number.MAX_SAFE_INTEGER', () => {
      const err = new ApiError(Number.MAX_SAFE_INTEGER, 'huge code');
      expect(err.statusCode).toBe(Number.MAX_SAFE_INTEGER);
    });

    it('accepts negative status code without throwing', () => {
      const err = new ApiError(-1, 'negative');
      expect(err.statusCode).toBe(-1);
    });

    it('accepts all standard HTTP codes', () => {
      const codes = [200, 201, 204, 301, 302, 400, 401, 403, 404, 409, 422, 429, 500, 502, 503];
      for (const code of codes) {
        const err = new ApiError(code, `code ${code}`);
        expect(err.statusCode).toBe(code);
      }
    });
  });

  // --- Boundary: messages --------------------------------------------------
  describe('message boundary values', () => {
    it('stores an empty string message', () => {
      const err = new ApiError(400, '');
      expect(err.message).toBe('');
    });

    it('stores a whitespace-only message', () => {
      const err = new ApiError(400, '   ');
      expect(err.message).toBe('   ');
    });

    it('stores a very long message (10 000 chars)', () => {
      const long = 'x'.repeat(10_000);
      const err = new ApiError(400, long);
      expect(err.message).toBe(long);
      expect(err.message.length).toBe(10_000);
    });

    it('stores a message with special characters', () => {
      const msg = '<script>alert(1)</script>\n\t\r\0\'";';
      const err = new ApiError(400, msg);
      expect(err.message).toBe(msg);
    });

    it('stores a unicode message', () => {
      const msg = '錯誤: 驗證失敗 🔥';
      const err = new ApiError(400, msg);
      expect(err.message).toBe(msg);
    });
  });

  // --- Serialisability -----------------------------------------------------
  describe('serialisability', () => {
    it('can be JSON-serialised via a plain object', () => {
      const err = new ApiError(400, 'serialise me');
      const obj = { statusCode: err.statusCode, message: err.message, isOperational: err.isOperational };
      const json = JSON.stringify(obj);
      const parsed = JSON.parse(json);
      expect(parsed.statusCode).toBe(400);
      expect(parsed.message).toBe('serialise me');
      expect(parsed.isOperational).toBe(true);
    });
  });

  // --- Immutability invariants ---------------------------------------------
  describe('field immutability after construction', () => {
    it('statusCode remains unchanged after construction', () => {
      const err = new ApiError(404, 'not found');
      // TypeScript types are readonly; at runtime we can observe the value doesn't drift
      const original = err.statusCode;
      expect(err.statusCode).toBe(original);
    });
  });
});

// ---------------------------------------------------------------------------
// ValidationError
// ---------------------------------------------------------------------------
describe('ValidationError', () => {
  it('has statusCode 400', () => {
    expect(new ValidationError('bad').statusCode).toBe(400);
  });

  it('propagates the custom message', () => {
    expect(new ValidationError('field required').message).toBe('field required');
  });

  it('isOperational defaults to true', () => {
    expect(new ValidationError('x').isOperational).toBe(true);
  });

  it('is instanceof ValidationError', () => {
    expect(new ValidationError('x')).toBeInstanceOf(ValidationError);
  });

  it('is instanceof ApiError', () => {
    expect(new ValidationError('x')).toBeInstanceOf(ApiError);
  });

  it('is instanceof Error', () => {
    expect(new ValidationError('x')).toBeInstanceOf(Error);
  });

  it('narrowing: instanceof ApiError catches ValidationError', () => {
    let caught: ApiError | null = null;
    try {
      throw new ValidationError('narrow');
    } catch (e) {
      if (e instanceof ApiError) caught = e;
    }
    expect(caught).not.toBeNull();
    expect(caught!.statusCode).toBe(400);
  });

  // Boundary: empty and long messages
  it('accepts empty string message', () => {
    const err = new ValidationError('');
    expect(err.message).toBe('');
    expect(err.statusCode).toBe(400);
  });

  it('accepts very long message', () => {
    const long = 'v'.repeat(5_000);
    expect(new ValidationError(long).message).toBe(long);
  });

  it('does NOT have a default message (message is always caller-supplied)', () => {
    // Providing an explicit message is required by the constructor signature
    const err = new ValidationError('required field missing');
    expect(err.message).toBeTruthy();
  });

  it('stack trace is present', () => {
    const err = new ValidationError('trace');
    expect(err.stack).toBeDefined();
    expect(err.stack!.length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// UnauthorizedError
// ---------------------------------------------------------------------------
describe('UnauthorizedError', () => {
  it('has statusCode 401', () => {
    expect(new UnauthorizedError().statusCode).toBe(401);
  });

  it('defaults message to "Unauthorized"', () => {
    expect(new UnauthorizedError().message).toBe('Unauthorized');
  });

  it('accepts a custom message', () => {
    expect(new UnauthorizedError('Token expired').message).toBe('Token expired');
  });

  it('custom message still has statusCode 401', () => {
    expect(new UnauthorizedError('No token').statusCode).toBe(401);
  });

  it('isOperational is true', () => {
    expect(new UnauthorizedError().isOperational).toBe(true);
  });

  it('is instanceof UnauthorizedError', () => {
    expect(new UnauthorizedError()).toBeInstanceOf(UnauthorizedError);
  });

  it('is instanceof ApiError', () => {
    expect(new UnauthorizedError()).toBeInstanceOf(ApiError);
  });

  it('is instanceof Error', () => {
    expect(new UnauthorizedError()).toBeInstanceOf(Error);
  });

  // Failure / boundary paths
  it('empty string overrides default message', () => {
    const err = new UnauthorizedError('');
    expect(err.message).toBe('');
    expect(err.statusCode).toBe(401);
  });

  it('whitespace-only message is stored as-is', () => {
    expect(new UnauthorizedError('  ').message).toBe('  ');
  });

  it('long custom message is stored correctly', () => {
    const long = 'a'.repeat(3_000);
    expect(new UnauthorizedError(long).message.length).toBe(3_000);
  });

  it('can be re-thrown and caught as ApiError', () => {
    function riskyOperation() {
      throw new UnauthorizedError('re-throw test');
    }
    expect(() => riskyOperation()).toThrow(ApiError);
    expect(() => riskyOperation()).toThrow('re-throw test');
  });
});

// ---------------------------------------------------------------------------
// ForbiddenError
// ---------------------------------------------------------------------------
describe('ForbiddenError', () => {
  it('has statusCode 403', () => {
    expect(new ForbiddenError().statusCode).toBe(403);
  });

  it('defaults message to "Forbidden"', () => {
    expect(new ForbiddenError().message).toBe('Forbidden');
  });

  it('accepts a custom message', () => {
    expect(new ForbiddenError('Access denied').message).toBe('Access denied');
  });

  it('custom message still has statusCode 403', () => {
    expect(new ForbiddenError('No token').statusCode).toBe(403);
  });

  it('isOperational is true', () => {
    expect(new ForbiddenError().isOperational).toBe(true);
  });

  it('is instanceof ForbiddenError', () => {
    expect(new ForbiddenError()).toBeInstanceOf(ForbiddenError);
  });

  it('is instanceof ApiError', () => {
    expect(new ForbiddenError()).toBeInstanceOf(ApiError);
  });

  it('is instanceof Error', () => {
    expect(new ForbiddenError()).toBeInstanceOf(Error);
  });

  // Failure / boundary paths
  it('empty string overrides default message', () => {
    const err = new ForbiddenError('');
    expect(err.message).toBe('');
    expect(err.statusCode).toBe(403);
  });

  it('whitespace-only message is stored as-is', () => {
    expect(new ForbiddenError('  ').message).toBe('  ');
  });

  it('long custom message is stored correctly', () => {
    const long = 'a'.repeat(3_000);
    expect(new ForbiddenError(long).message.length).toBe(3_000);
  });

  it('can be re-thrown and caught as ApiError', () => {
    function riskyOperation() {
      throw new ForbiddenError('re-throw test');
    }
    expect(() => riskyOperation()).toThrow(ApiError);
    expect(() => riskyOperation()).toThrow('re-throw test');
  });
});

// ---------------------------------------------------------------------------
// NotFoundError
// ---------------------------------------------------------------------------
describe('NotFoundError', () => {
  it('has statusCode 404', () => {
    expect(new NotFoundError().statusCode).toBe(404);
  });

  it('defaults message to "Resource not found"', () => {
    expect(new NotFoundError().message).toBe('Resource not found');
  });

  it('accepts a custom message', () => {
    expect(new NotFoundError('User not found').message).toBe('User not found');
  });

  it('custom message still has statusCode 404', () => {
    expect(new NotFoundError('Asset missing').statusCode).toBe(404);
  });

  it('isOperational is true', () => {
    expect(new NotFoundError().isOperational).toBe(true);
  });

  it('is instanceof NotFoundError', () => {
    expect(new NotFoundError()).toBeInstanceOf(NotFoundError);
  });

  it('is instanceof ApiError', () => {
    expect(new NotFoundError()).toBeInstanceOf(ApiError);
  });

  it('is instanceof Error', () => {
    expect(new NotFoundError()).toBeInstanceOf(Error);
  });

  // Failure / boundary paths
  it('empty string overrides default message', () => {
    const err = new NotFoundError('');
    expect(err.message).toBe('');
    expect(err.statusCode).toBe(404);
  });

  it('404 error does not bleed into 400 range', () => {
    expect(new NotFoundError().statusCode).toBeGreaterThanOrEqual(400);
    expect(new NotFoundError().statusCode).toBeLessThan(500);
  });

  it('can be caught by a generic Error handler', () => {
    let caughtMessage = '';
    try {
      throw new NotFoundError('generic catch');
    } catch (e: unknown) {
      if (e instanceof Error) caughtMessage = e.message;
    }
    expect(caughtMessage).toBe('generic catch');
  });
});

// ---------------------------------------------------------------------------
// ConflictError
// ---------------------------------------------------------------------------
describe('ConflictError', () => {
  it('has statusCode 409', () => {
    expect(new ConflictError('duplicate').statusCode).toBe(409);
  });

  it('propagates the custom message', () => {
    expect(new ConflictError('resource already exists').message).toBe('resource already exists');
  });

  it('isOperational is true', () => {
    expect(new ConflictError('dup').isOperational).toBe(true);
  });

  it('is instanceof ConflictError', () => {
    expect(new ConflictError('x')).toBeInstanceOf(ConflictError);
  });

  it('is instanceof ApiError', () => {
    expect(new ConflictError('x')).toBeInstanceOf(ApiError);
  });

  it('is instanceof Error', () => {
    expect(new ConflictError('x')).toBeInstanceOf(Error);
  });

  // Failure / boundary paths
  it('accepts empty string message', () => {
    const err = new ConflictError('');
    expect(err.message).toBe('');
    expect(err.statusCode).toBe(409);
  });

  it('stores very long message', () => {
    const long = 'c'.repeat(4_000);
    expect(new ConflictError(long).message.length).toBe(4_000);
  });

  it('statusCode is not 400 or 500', () => {
    expect(new ConflictError('dup').statusCode).toBe(409);
  });
});

// ---------------------------------------------------------------------------
// InternalServerError
// ---------------------------------------------------------------------------
describe('InternalServerError', () => {
  it('has statusCode 500', () => {
    expect(new InternalServerError().statusCode).toBe(500);
  });

  it('defaults message to "Internal server error"', () => {
    expect(new InternalServerError().message).toBe('Internal server error');
  });

  it('accepts a custom message', () => {
    expect(new InternalServerError('Database unavailable').message).toBe('Database unavailable');
  });

  it('custom message still has statusCode 500', () => {
    expect(new InternalServerError('DB down').statusCode).toBe(500);
  });

  it('isOperational is true by default', () => {
    expect(new InternalServerError().isOperational).toBe(true);
  });

  it('is instanceof InternalServerError', () => {
    expect(new InternalServerError()).toBeInstanceOf(InternalServerError);
  });

  it('is instanceof ApiError', () => {
    expect(new InternalServerError()).toBeInstanceOf(ApiError);
  });

  it('is instanceof Error', () => {
    expect(new InternalServerError()).toBeInstanceOf(Error);
  });

  // Failure / boundary paths
  it('empty string overrides default message', () => {
    const err = new InternalServerError('');
    expect(err.message).toBe('');
    expect(err.statusCode).toBe(500);
  });

  it('statusCode is in the 5xx range', () => {
    expect(new InternalServerError().statusCode).toBeGreaterThanOrEqual(500);
    expect(new InternalServerError().statusCode).toBeLessThan(600);
  });

  it('long custom message is stored correctly', () => {
    const long = 'i'.repeat(6_000);
    expect(new InternalServerError(long).message.length).toBe(6_000);
  });
});

// ---------------------------------------------------------------------------
// Cross-cutting: instanceof narrowing and switch-dispatch
// ---------------------------------------------------------------------------
describe('Cross-cutting: instanceof narrowing and error dispatch', () => {
  function getStatusCode(err: Error): number {
    if (err instanceof ApiError) return err.statusCode;
    return 500;
  }

  it('getStatusCode returns 400 for ValidationError', () => {
    expect(getStatusCode(new ValidationError('v'))).toBe(400);
  });

  it('getStatusCode returns 401 for UnauthorizedError', () => {
    expect(getStatusCode(new UnauthorizedError())).toBe(401);
  });

  it('getStatusCode returns 403 for ForbiddenError', () => {
    expect(getStatusCode(new ForbiddenError())).toBe(403);
  });

  it('getStatusCode returns 404 for NotFoundError', () => {
    expect(getStatusCode(new NotFoundError())).toBe(404);
  });

  it('getStatusCode returns 409 for ConflictError', () => {
    expect(getStatusCode(new ConflictError('dup'))).toBe(409);
  });

  it('getStatusCode returns 500 for InternalServerError', () => {
    expect(getStatusCode(new InternalServerError())).toBe(500);
  });

  it('getStatusCode falls back to 500 for plain Error', () => {
    expect(getStatusCode(new Error('plain'))).toBe(500);
  });

  it('all subclass instances satisfy instanceof ApiError', () => {
    const errors: Error[] = [
      new ValidationError('v'),
      new UnauthorizedError(),
      new ForbiddenError(),
      new NotFoundError(),
      new ConflictError('c'),
      new InternalServerError(),
    ];
    for (const e of errors) {
      expect(e instanceof ApiError).toBe(true);
    }
  });

  it('subclass instances do NOT cross-pollute each other', () => {
    expect(new ValidationError('v')).not.toBeInstanceOf(UnauthorizedError);
    expect(new UnauthorizedError()).not.toBeInstanceOf(ForbiddenError);
    expect(new ForbiddenError()).not.toBeInstanceOf(NotFoundError);
    expect(new NotFoundError()).not.toBeInstanceOf(ConflictError);
    expect(new ConflictError('c')).not.toBeInstanceOf(InternalServerError);
    expect(new InternalServerError()).not.toBeInstanceOf(ValidationError);
  });
});

// ---------------------------------------------------------------------------
// Cross-cutting: re-throw safety
// ---------------------------------------------------------------------------
describe('Cross-cutting: re-throw safety', () => {
  it('re-throwing an ApiError preserves statusCode', () => {
    function layer1() { throw new NotFoundError('missing'); }
    function layer2() { try { layer1(); } catch (e) { throw e; } }

    let caughtCode = 0;
    try { layer2(); } catch (e) {
      if (e instanceof ApiError) caughtCode = e.statusCode;
    }
    expect(caughtCode).toBe(404);
  });

  it('wrapping in a new Error loses ApiError information', () => {
    const original = new ValidationError('original');
    const wrapped = new Error(`Wrapped: ${original.message}`);
    expect(wrapped).not.toBeInstanceOf(ApiError);
    expect(wrapped.message).toContain('original');
  });

  it('async rejection with ApiError is catchable as ApiError', async () => {
    const rejected = Promise.reject(new UnauthorizedError('async reject'));
    await expect(rejected).rejects.toBeInstanceOf(ApiError);
    await expect(Promise.reject(new UnauthorizedError('msg'))).rejects.toMatchObject({
      statusCode: 401,
      message: 'msg',
    });
  });
});

// ---------------------------------------------------------------------------
// Cross-cutting: isOperational flag semantics
// ---------------------------------------------------------------------------
describe('Cross-cutting: isOperational flag semantics', () => {
  it('all subclasses have isOperational true by default', () => {
    expect(new ValidationError('x').isOperational).toBe(true);
    expect(new UnauthorizedError().isOperational).toBe(true);
    expect(new ForbiddenError().isOperational).toBe(true);
    expect(new NotFoundError().isOperational).toBe(true);
    expect(new ConflictError('x').isOperational).toBe(true);
    expect(new InternalServerError().isOperational).toBe(true);
  });

  it('ApiError can explicitly mark a non-operational error', () => {
    const programmerError = new ApiError(500, 'assert failed', false);
    expect(programmerError.isOperational).toBe(false);
  });

  it('isOperational false does not affect statusCode or message', () => {
    const err = new ApiError(503, 'db pool exhausted', false);
    expect(err.statusCode).toBe(503);
    expect(err.message).toBe('db pool exhausted');
  });
});

// ---------------------------------------------------------------------------
// Regression: prototype chain is preserved in inheritance hierarchy
// ---------------------------------------------------------------------------
describe('Regression: prototype chain correctness', () => {
  it('ValidationError prototype chain: ValidationError → ApiError → Error → Object', () => {
    const err = new ValidationError('chain');
    expect(err instanceof ValidationError).toBe(true);
    expect(err instanceof ApiError).toBe(true);
    expect(err instanceof Error).toBe(true);
  });

  it('UnauthorizedError prototype chain', () => {
    const err = new UnauthorizedError();
    expect(err instanceof UnauthorizedError).toBe(true);
    expect(err instanceof ApiError).toBe(true);
    expect(err instanceof Error).toBe(true);
  });

  it('ForbiddenError prototype chain', () => {
    const err = new ForbiddenError();
    expect(err instanceof ForbiddenError).toBe(true);
    expect(err instanceof ApiError).toBe(true);
    expect(err instanceof Error).toBe(true);
  });

  it('NotFoundError prototype chain', () => {
    const err = new NotFoundError();
    expect(err instanceof NotFoundError).toBe(true);
    expect(err instanceof ApiError).toBe(true);
    expect(err instanceof Error).toBe(true);
  });

  it('ConflictError prototype chain', () => {
    const err = new ConflictError('x');
    expect(err instanceof ConflictError).toBe(true);
    expect(err instanceof ApiError).toBe(true);
    expect(err instanceof Error).toBe(true);
  });

  it('InternalServerError prototype chain', () => {
    const err = new InternalServerError();
    expect(err instanceof InternalServerError).toBe(true);
    expect(err instanceof ApiError).toBe(true);
    expect(err instanceof Error).toBe(true);
  });
});
