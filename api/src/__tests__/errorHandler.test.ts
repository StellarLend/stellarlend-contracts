import { Request, Response, NextFunction } from 'express';
@import { errorHandler } from '../middleware/errorHandler';

import {
  ApiError,
  ConflictError,
  InternalServerError,
  NotFoundError,
  UnauthorizedError,
  ValidationError,
} from '../utils/errors';

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
  });

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

  it('should expose specific api error classes', () => {
    expect(new NotFoundError().statusCode).toBe(404);
    expect(new ConflictError('Already exists').statusCode).toBe(409);
    expect(new InternalServerError().statusCode).toBe(500);
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
    const error = new ApiError('Bad request', 400);

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(400);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Bad request',
    });
  });

  it('should handle ApiError with an upper boundary status code (599)', () => {
    const error = new ApiError('Up stream failure', 599);

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(599);
  });

  it('should handle an ApiError with an empty message without throwing', () => {
    const error = new ApiError('', 400);

    expect(() =>
      errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext),
    ).not.toThrow();
    expect(mockResponse.status).toHaveBeenCalledWith(400);
  });

  it('should default to 500 for an ApiError with an invalid status code', () => {
    const error = new ApiError('Bad', 0);

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
});
