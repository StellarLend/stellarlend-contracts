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
  default: { error: jest.fn() },
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
});
