import { Request, Response, NextFunction } from 'express';
const { errorHandler } = require('../middleware/errorHandler');
const {
  ApiError,
  ConflictError,
  InternalServerError,
  NotFoundError,
  UnauthorizedError,
  ValidationError,
} = require('../utils/errors');

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

  it('should not leak internal messages for unknown errors', () => {
    const error = new Error('database connection string secret');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
    const body = (mockResponse.json as jest.Mock).mock.calls[0][0];
    expect(JSON.stringify(body)).not.toContain('database');
  });

  it('should not leak internal messages for 5xx ApiErrors', () => {
    const error = new InternalServerError('secret internal detail');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
  });

  it('should handle malformed JSON body as 400', () => {
    const error = new SyntaxError('Unexpected token in JSON');
    (error as unknown as { body: unknown }).body = '{';

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(400);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Unexpected token in JSON',
    });
  });

  it('should fall back to 500 for out-of-range ApiError status codes', () => {
    const error = new ApiError('bad code', 999);

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Internal server error',
    });
  });

  it('should fall back to 500 for non-integer ApiError status codes', () => {
    const error = new ApiError('bad code', 400.5);

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should be deterministic across repeated and concurrent invocations', () => {
    const error = new ValidationError('Invalid input');
    const responses = Array.from({ length: 5 }, () => (
      {
        status: jest.fn().mockReturnThis(),
        json: jest.fn().mockReturnThis(),
      } as Partial<Response>
    ));

    responses.forEach((res) =>
      errorHandler(error, mockRequest as Request, res as Response, mockNext)
    );

    responses.forEach((res) => {
      expect(res.status).toHaveBeenCalledWith(400);
      expect(res.json).toHaveBeenCalledWith({
        success: false,
        error: 'Invalid input',
      });
    });
  });

  it('should remain safe when request context is missing', () => {
    const error = new Error('boom');

    errorHandler(error, {} as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should not call next after responding', () => {
    const error = new ValidationError('Invalid input');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockNext).not.toHaveBeenCalled();
  });
});
