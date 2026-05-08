// ========== Auth Types ==========
export interface User {
  username: string;
}

export interface AuthTokenPair {
  refresh_token: string;
  access_token: string;
}
