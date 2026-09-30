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

jest.mock('../utils/logger', () => ({
  __esModule: true,
  default: { error: jest.fn(), info: jest.fn(), warn: jest.fn(), debug: jest.fn() },
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
      headersSent: false,
    };
    mockNext = jest.fn();
    (logger.error as jest.Mock).mockClear();
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
    expect(new InternalServerError().statusCode).toBe(undefined);
  });

  it('should handle JSON parsing SyntaxError with 400', () => {
    const error = new SyntaxError('Unexpected token in JSON');
    (error as SyntaxError & { body?: unknown }).body = '{ invalid }';

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(400);
    expect(mockResponse.json).toHaveBeenCalledWith({
      success: false,
      error: 'Unexpected token in JSON',
    });
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
    const error = new ApiError('Weird code', 999);

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should clamp negative ApiError status codes to 500', () => {
    const error = new ApiError('Negative', -1);

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

  it('should not throw when receiving null', () => {
    errorHandler(null, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should not throw when receiving undefined', () => {
    errorHandler(undefined, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should not throw if logger fails', () => {
    (logger.error as jest.Mock).mockImplementationOnce(() => {
      throw new Error('logger failure');
    });

    const error = new Error('Boom');
    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledWith(500);
  });

  it('should be deterministic across repeated invocations', () => {
    const error = new ValidationError('Invalid input');

    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);
    errorHandler(error, mockRequest as Request, mockResponse as Response, mockNext);

    expect(mockResponse.status).toHaveBeenCalledTimes(2);
    expect(mockResponse.status).toHaveNthCalledWith(1, 400);
    expect(mockResponse.status).toHaveNthCalledWith(2, 400);
  });
});
