import { Request, Response, NextFunction } from 'express';

import { errorHandler } from '../middleware/errorHandler';
import logger from '../utils/logger';
import {
  ApiError,
  ConflictError,
  InternalServerError,
  NotFoundError,
  UnauthorizedError,
  ValidationError,
} from '../utils/errors';
import logger from '../utils/logger';

// Mock the logger to prevent actual logging during tests
jest.mock('../utils/logger', () => ({
  error: jest.fn(),
  warn: jest.fn(),
  info: jest.fn(),
  debug: jest.fn(),
}));

jest.mock('../utils/logger', () => ({
  __esModule: true,
  default: { error: jest.fn() },
}));

jest.mock('../utils/logger', () => ({
  __esModule: true,
  default: { error: jest.fn(), info: jest.fn(), warn: jest.fn(), debug: jest.fn() },
}));

describe('Error Handler Middleware', () => {
  let mockRequest: Partial<Request>;
  let mockResponse: Partial<Response>;
  let mockNext: NextFunction;

  beforeEach(() => {
    jest.clearAllMocks();
    mockRequest = {
      path: '/api/test',
      method: 'POST',
    };
    mockResponse = {
      status: jest.fn().mockReturnThis(),
      json: jest.fn().mockReturnThis(),
      headersSent: false,
    };
    mockNext = jest.fn();
    jest.clearAllMocks();
  });

  describe('Success Path - Standard ApiError Handling', () => {
    it('should handle ApiError with correct status code', () => {
      const error = new ValidationError('Invalid input');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Invalid input',
      });
    });

    it('should handle UnauthorizedError', () => {
      const error = new UnauthorizedError();

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(401);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Unauthorized',
      });
    });

    it('should expose specific api error classes', () => {
      expect(new NotFoundError().statusCode).toBe(404);
      expect(new ConflictError('Already exists').statusCode).toBe(409);
      expect(new InternalServerError().statusCode).toBe(500);
    });
  });

  describe('Failure Path - Invalid Inputs and Edge Cases', () => {
    it('should handle null error object gracefully', () => {
      const error = null as any;

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });

    it('should handle undefined error gracefully', () => {
      const error = undefined as any;

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });

    it('should handle error without message property', () => {
      const error = { statusCode: 400 } as any;

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });

    it('should handle error with empty message string', () => {
      const error = new ValidationError('');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: '',
      });
    });

    it('should handle error with very long message', () => {
      const longMessage = 'x'.repeat(10000);
      const error = new ValidationError(longMessage);

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: longMessage,
      });
    });

    it('should handle error with special characters in message', () => {
      const specialMessage = '<script>alert("xss")</script>\n\r\t\0';
      const error = new ValidationError(specialMessage);

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: specialMessage,
      });
    });

    it('should handle SyntaxError for malformed JSON body', () => {
      const syntaxError = new SyntaxError('Unexpected token } in JSON');
      (syntaxError as any).body = '{"invalid": }';

      errorHandler(syntaxError, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Unexpected token } in JSON',
      });
    });

    it('should handle generic errors with 500 status', () => {
      const error = new Error('Something went wrong');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });

    it('should handle TypeError gracefully', () => {
      const error = new TypeError('Cannot read property of undefined');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });

    it('should handle ReferenceError gracefully', () => {
      const error = new ReferenceError('Variable is not defined');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });
  });

  describe('Boundary Cases - Status Codes', () => {
    it('should handle NotFoundError with 404 status', () => {
      const error = new NotFoundError('User not found');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(404);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'User not found',
      });
    });

    it('should handle ConflictError with 409 status', () => {
      const error = new ConflictError('Resource already exists');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(409);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Resource already exists',
      });
    });

    it('should handle InternalServerError with 500 status', () => {
      const error = new InternalServerError('Database connection failed');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Database connection failed',
      });
    });

    it('should handle custom ApiError with arbitrary status code', () => {
      const error = new ApiError(418, "I'm a teapot");

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(418);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: "I'm a teapot",
      });
    });

    it('should handle boundary status code 100', () => {
      const error = new ApiError(100, 'Continue');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(100);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Continue',
      });
    });

    it('should handle boundary status code 599', () => {
      const error = new ApiError(599, 'Network Connect Timeout');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(599);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Network Connect Timeout',
      });
    });
  });

  describe('Boundary Cases - Request Properties', () => {
    it('should handle request without path property', () => {
      const error = new ValidationError('Invalid input');
      mockRequest = { method: 'GET' };

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Invalid input',
      });
    });

    it('should handle request without method property', () => {
      const error = new ValidationError('Invalid input');
      mockRequest = { path: '/api/test' };

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Invalid input',
      });
    });

    it('should handle completely empty request object', () => {
      const error = new ValidationError('Invalid input');
      mockRequest = {};

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Invalid input',
      });
    });

    it('should handle request with very long path', () => {
      const error = new ValidationError('Invalid input');
      mockRequest = {
        path: '/api/' + 'x'.repeat(5000),
        method: 'POST',
      };

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(400);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Invalid input',
      });
    });
  });

  describe('Logging Behavior', () => {
    it('should log error with request context', () => {
      const error = new ValidationError('Invalid input');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(logger.error).toHaveBeenCalledWith('Error occurred:', {
        error: 'Invalid input',
        stack: expect.any(String),
        path: '/api/test',
        method: 'POST',
      });
    });

    it('should log error even when error has no stack trace', () => {
      const error = new ValidationError('Invalid input');
      error.stack = undefined;

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(logger.error).toHaveBeenCalledWith('Error occurred:', {
        error: 'Invalid input',
        stack: undefined,
        path: '/api/test',
        method: 'POST',
      });
    });

    it('should log SyntaxError with proper context', () => {
      const syntaxError = new SyntaxError('Unexpected token');
      (syntaxError as any).body = '{"bad": }';

      errorHandler(syntaxError, mockRequest as Request, mockResponse as Response, mockNext);

      expect(logger.error).toHaveBeenCalledWith('Error occurred:', {
        error: 'Unexpected token',
        stack: expect.any(String),
        path: '/api/test',
        method: 'POST',
      });
    });

    it('should log generic Error with proper context', () => {
      const error = new Error('Unexpected error');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(logger.error).toHaveBeenCalledWith('Error occurred:', {
        error: 'Unexpected error',
        stack: expect.any(String),
        path: '/api/test',
        method: 'POST',
      });
    });
  });

  describe('Response Invariants', () => {
    it('should always return success: false for errors', () => {
      const errors = [
        new ValidationError('Invalid'),
        new UnauthorizedError(),
        new NotFoundError(),
        new ConflictError('Conflict'),
        new InternalServerError(),
        new Error('Generic error'),
      ];

      errors.forEach((error) => {
        jest.clearAllMocks();
        errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
        expect(mockResponse.json).toHaveBeenCalledWith(
          expect.objectContaining({ success: false })
        );
      });
    });

    it('should never expose internal error details for generic errors', () => {
      const error = new Error('Database password is: secret123');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
      // Verify sensitive information is not exposed
      expect(mockResponse.json).not.toHaveBeenCalledWith(
        expect.objectContaining({ error: expect.stringContaining('secret123') })
      );
    });

    it('should always call status before json', () => {
      const error = new ValidationError('Invalid');
      const callOrder: string[] = [];

      mockResponse.status = jest.fn().mockImplementation(() => {
        callOrder.push('status');
        return mockResponse;
      });
      mockResponse.json = jest.fn().mockImplementation(() => {
        callOrder.push('json');
        return mockResponse;
      });

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(callOrder).toEqual(['status', 'json']);
    });

    it('should not call next function', () => {
      const error = new ValidationError('Invalid');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('Concurrent and Duplicate Execution', () => {
    it('should handle multiple errors in rapid succession', () => {
      const errors = [
        new ValidationError('Error 1'),
        new UnauthorizedError('Error 2'),
        new NotFoundError('Error 3'),
      ];

      errors.forEach((error) => {
        errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
      });

      expect(mockResponse.status).toHaveBeenCalledTimes(3);
      expect(mockResponse.json).toHaveBeenCalledTimes(3);
    });

    it('should handle same error instance multiple times', () => {
      const error = new ValidationError('Duplicate error');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledTimes(2);
      expect(mockResponse.json).toHaveBeenCalledTimes(2);
    });
  });

  describe('Error Type Discrimination', () => {
    it('should distinguish SyntaxError with body property from generic SyntaxError', () => {
      const syntaxErrorWithBody = new SyntaxError('Parse error');
      (syntaxErrorWithBody as any).body = '{"invalid"}';

      errorHandler(
        syntaxErrorWithBody,
        mockRequest as Request,
        mockResponse as Response,
        mockNext
      );

      expect(mockResponse.status).toHaveBeenCalledWith(400);
    });

    it('should treat SyntaxError without body as generic error', () => {
      const syntaxErrorWithoutBody = new SyntaxError('Parse error');

      errorHandler(
        syntaxErrorWithoutBody,
        mockRequest as Request,
        mockResponse as Response,
        mockNext
      );

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });

    it('should correctly identify ApiError instances', () => {
      const apiError = new ApiError(403, 'Forbidden');

      errorHandler(apiError, mockRequest as Request, mockResponse as Response, mockNext);

      expect(mockResponse.status).toHaveBeenCalledWith(403);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Forbidden',
      });
    });

    it('should handle error that looks like ApiError but is not', () => {
      const fakeApiError = {
        statusCode: 400,
        message: 'Fake error',
        name: 'Error',
      } as any;

      errorHandler(
        fakeApiError,
        mockRequest as Request,
        mockResponse as Response,
        mockNext
      );

      expect(mockResponse.status).toHaveBeenCalledWith(500);
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Internal server error',
      });
    });
  });

  describe('Error Class Properties and Invariants', () => {
    it('should verify ApiError extends Error', () => {
      const error = new ApiError(400, 'Test');
      expect(error instanceof Error).toBe(true);
      expect(error instanceof ApiError).toBe(true);
    });

    it('should verify all custom errors extend ApiError', () => {
      const errors = [
        new ValidationError('test'),
        new UnauthorizedError(),
        new NotFoundError(),
        new ConflictError('test'),
        new InternalServerError(),
      ];

      errors.forEach((error) => {
        expect(error instanceof ApiError).toBe(true);
        expect(error instanceof Error).toBe(true);
      });
    });

    it('should verify ApiError default isOperational flag', () => {
      const error = new ApiError(400, 'Test');
      expect(error.isOperational).toBe(true);
    });

    it('should allow non-operational errors', () => {
      const error = new ApiError(500, 'Critical failure', false);
      expect(error.isOperational).toBe(false);
    });

    it('should preserve error message through inheritance chain', () => {
      const error = new ValidationError('Validation failed');
      expect(error.message).toBe('Validation failed');
      expect(error.statusCode).toBe(400);
    });

    it('should use default messages for errors that provide them', () => {
      expect(new UnauthorizedError().message).toBe('Unauthorized');
      expect(new NotFoundError().message).toBe('Resource not found');
      expect(new InternalServerError().message).toBe('Internal server error');
    });

    it('should allow custom messages for errors with defaults', () => {
      expect(new UnauthorizedError('Custom auth message').message).toBe('Custom auth message');
      expect(new NotFoundError('Custom not found message').message).toBe(
        'Custom not found message'
      );
      expect(new InternalServerError('Custom server error').message).toBe(
        'Custom server error'
      );
    });
  });

  describe('Regression Prevention', () => {
    it('should maintain backward compatibility with existing error responses', () => {
      const error = new ValidationError('Field is required');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      // Ensure response structure matches expected format
      expect(mockResponse.json).toHaveBeenCalledWith({
        success: false,
        error: 'Field is required',
      });
      // Ensure no additional fields are added
      expect(mockResponse.json).toHaveBeenCalledWith(
        expect.not.objectContaining({
          statusCode: expect.anything(),
          stack: expect.anything(),
          timestamp: expect.anything(),
        })
      );
    });

    it('should not leak stack traces to client in production-like scenarios', () => {
      const error = new Error('Internal error with sensitive stack');

      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

      const jsonCall = (mockResponse.json as jest.Mock).mock.calls[0][0];
      expect(jsonCall).not.toHaveProperty('stack');
      expect(jsonCall.error).toBe('Internal server error');
    });
  });

  it.each([
    [new ValidationError('Invalid input'), 400, 'Invalid input'],
    [new UnauthorizedError('Access token required'), 401, 'Access token required'],
    [new NotFoundError(), 404, 'Resource not found'],
    [new ConflictError('Already exists'), 409, 'Already exists'],
  ])('preserves the public response for %p', (error, status, message) => {
    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(status);
    expect(mockResponse.json).toHaveBeenCalledWith({ success: false, error: message });
    expect(mockNext).not.toHaveBeenCalled();
  });

  it('does not echo a malformed JSON body in the response or log', () => {
    const error = Object.assign(new SyntaxError('Unexpected private-input-value'), {
      body: '{"value":"private-input-value"}',
    });

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(400);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Invalid JSON body',
    });
    expect(logger.error).toHaveBeenCalledWith('Request failed', {
      category: 'invalid_json',
      statusCode: 400,
    });
    expect(JSON.stringify((logger.error as jest.Mock).mock.calls)).not.toContain(
      'private-input-value'
    );
  });

  it('does not log or return unexpected error details', () => {
    mockRequest = { path: '/private-input-value', method: 'private-input-value' };
    errorHandler(
      new Error('private-input-value'),
      mockRequest as Request,
      mockResponse as Response,
      mockNext
    );

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
    expect(logger.error).toHaveBeenCalledWith('Request failed', {
      category: 'server_error',
      statusCode: 500,
    });
    expect(JSON.stringify((logger.error as jest.Mock).mock.calls)).not.toContain(
      'private-input-value'
    );
  });

  it('handles a non-Error rejection without exposing its fields', () => {
    errorHandler(
      { message: 'private-input-value' } as Error,
      mockRequest as Request,
      mockResponse as Response,
      mockNext
    );

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
    expect(JSON.stringify((logger.error as jest.Mock).mock.calls)).not.toContain(
      'private-input-value'
    );
  });

  it.each([0, 200, 600, NaN])('rejects an invalid ApiError status %p', (status) => {
    errorHandler(
      new ApiError(status, 'private-input-value'),
      mockRequest as Request,
      mockResponse as Response,
      mockNext
    );

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
  });

  it('hides non-operational and server error messages', () => {
    for (const error of [
      new ApiError(400, 'private-input-value', false),
      new ApiError(500, 'private-input-value'),
    ]) {
      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
    }

    expect((mockResponse.status as jest.Mock).mock.calls).toEqual([[500], [500]]);
    expect((mockResponse.json as jest.Mock).mock.calls).toEqual([
      [{ success: false, error: 'Internal server error' }],
      [{ success: false, error: 'Internal server error' }],
    ]);
  });

  it('delegates after headers have been sent without writing twice', () => {
    const error = new UnauthorizedError();
    mockResponse.headersSent = true;

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockNext).toHaveBeenCalledWith(error);
    expect(mockResponse.status).not.toHaveBeenCalled();
    expect(mockResponse.json).not.toHaveBeenCalled();
  });

  it('delegates a non-Error rejection safely after headers have been sent', () => {
    mockResponse.headersSent = true;

    errorHandler(
      { message: 'private-input-value' } as Error,
      mockRequest as Request,
      mockResponse as Response,
      mockNext
    );

    expect(mockNext).toHaveBeenCalledWith(new Error('Unknown error'));
    expect(mockResponse.status).not.toHaveBeenCalled();
    expect(mockResponse.json).not.toHaveBeenCalled();
  });

  it('keeps repeated and independent failures isolated', () => {
    const first = new UnauthorizedError();
    const second = new Error('private-input-value');

    errorHandler(first, mockRequest as Request, mockResponse as Response, mockNext);
    errorHandler(second, mockRequest as Request, mockResponse as Response, mockNext);
    errorHandler(first, mockRequest as Request, mockResponse as Response, mockNext);

    expect((mockResponse.status as jest.Mock).mock.calls).toEqual([[401], [500], [401]]);
    expect((mockResponse.json as jest.Mock).mock.calls).toEqual([
      [{ success: false, error: 'Unauthorized' }],
      [{ success: false, error: 'Internal server error' }],
      [{ success: false, error: 'Unauthorized' }],
    ]);
  });

  // --- Failure-path and boundary coverage ---

  it('should not leak sensitive details for generic errors', () => {
    const secret = 'secret-token-abc123';
    const error = new Error(`Database failure: ${secret}`);

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
    const body = (mockResponse.json as jest.Mock).mock.calls[0][0] as { error: string };
    expect(body.error).not.toContain(secret);
  });

  it('should handle ApiError with a minimal boundary status code (400)', () => {
    const error = new ApiError(400, 'Bad request');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(400);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Bad request',
    });
  });

  it('should default to 500 for an ApiError with an invalid status code', () => {
    const error = new ApiError(0, 'Bad');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should not invoke next once a response has been sent', () => {
    const error = new ValidationError('Invalid input');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockNext).not.toHaveBeenCalled();
  });

  it('should handle a generic error with an empty message', () => {
    const error = new Error('');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
  });

  it('should handle a non-Error thrown value deterministically', () => {
    const error = { message: 'not an error instance' } as unknown as Error;

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
  });

  it('should expose specific api error classes', () => {
    expect(new NotFoundError().statusCode).toBe(404);
    expect(new ConflictError('Already exists').statusCode).toBe(409);
    expect(new InternalServerError().statusCode).toBe(500);
  });

  it('should not treat a plain SyntaxError without body as a 400', () => {
    const error = new SyntaxError('Bad syntax');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should not leak internal error messages for generic errors', () => {
    const error = new Error('DB: connection refused at 10.0.0.1');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
    expect(mockResponse.json).not.toHaveBeenCalledWith(
        expect.objectContaining({ error: expect.stringContaining('10.0.0.1') }),
    );
  });

  it('should clamp out-of-range ApiError status codes to 500', () => {
    const error = new ApiError(999, 'Weird code');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should clamp negative ApiError status codes to 500', () => {
    const error = new ApiError(-1, 'Negative');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should delegate to next when headers are already sent', () => {
    mockResponse.headersSent = true;
    const error = new Error('Late failure');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockNext).toHaveBeenCalledWith(error);
    expect(mockResponse.status).not.toHaveBeenCalled();
    expect(mockResponse.json).not.toHaveBeenCalled();
  });

  it('should not throw when receiving a non-Error thrown value', () => {
    errorHandler(
      'string failure' as unknown,
      mockRequest as Request,
      mockResponse as Response,
      mockNext,
    );

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
  });

  it('should produce the same response for repeated invocations with the same input (determinism)', () => {
    const error = new ConflictError('Already exists');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    const statusCalls = (mockResponse.status as jest.Mock).mock.calls;
    const jsonCalls = (mockResponse.json as jest.Mock).mock.calls;
    expect(statusCalls).toHaveLength(2);
    expect(statusCalls[0][0]).toBe(409);
    expect(statusCalls[1][0]).toBe(409);
    expect(jsonCalls[0][0]).toEqual(jsonCalls[1][0]);
  });

  it('should not mutate the incoming error object', () => {
    const error = new ValidationError('Invalid input');
    const snapshot = {
      message: error.message,
      statusCode: error.statusCode,
    };

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(error.message).toBe(snapshot.message);
    expect(error.statusCode).toBe(snapshot.statusCode);
  });

  it('should not throw when the response is already headers-sent (concurrent send)', () => {
    const error = new ValidationError('Invalid input');
    (mockResponse as { headersSent?: boolean }).headersSent = true;

    expect(() =>
      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext),
    ).not.toThrow();
  });

  it('should handle a 404 NotFoundError with the correct body', () => {
    const error = new NotFoundError('Resource missing');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(404);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Resource missing',
    });
  });

  it('should not throw when receiving null', () => {
    errorHandler(null, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should not throw when receiving undefined', () => {
    errorHandler(undefined, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should be deterministic across repeated invocations', () => {
    const error = new ValidationError('Invalid input');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledTimes(2);
    expect(mockResponse.status).toHaveBeenNthCalledWith(1, 400);
    expect(mockResponse.status).toHaveBeenNthCalledWith(2, 400);
  });
});
