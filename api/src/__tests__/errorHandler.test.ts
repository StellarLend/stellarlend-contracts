import { Request, Response, NextFunction } from 'express';
import { errorHandler } from '../middleware/errorHandler';
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

describe('Error Handler Middleware', () => {
  let mockRequest: Partial<Request>;
  let mockResponse: Partial<Response>;
  let mockNext: NextFunction;

  beforeEach(() => {
    mockRequest = {
      path: '/api/test',
      method: 'POST',
    };
    mockResponse = {
      status: jest.fn().mockReturnThis(),
      json: jest.fn().mockReturnThis(),
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
});
