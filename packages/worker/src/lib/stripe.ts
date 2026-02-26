import Stripe from 'stripe';

let stripeClient: Stripe | null = null;

export function getStripe(secretKey: string): Stripe {
  if (!stripeClient) {
    stripeClient = new Stripe(secretKey, {
      apiVersion: '2024-12-18.acacia',
    });
  }
  return stripeClient;
}

export async function createCheckoutSession(
  stripe: Stripe,
  customerId: string,
  successUrl: string,
  cancelUrl: string
): Promise<string> {
  const session = await stripe.checkout.sessions.create({
    customer: customerId,
    mode: 'subscription',
    line_items: [
      {
        price_data: {
          currency: 'usd',
          product_data: { name: 'NetMon Pro' },
          unit_amount: 300,
          recurring: { interval: 'month' },
        },
        quantity: 1,
      },
    ],
    success_url: successUrl,
    cancel_url: cancelUrl,
  });
  return session.url!;
}

export async function createPortalSession(
  stripe: Stripe,
  customerId: string,
  returnUrl: string
): Promise<string> {
  const session = await stripe.billingPortal.sessions.create({
    customer: customerId,
    return_url: returnUrl,
  });
  return session.url;
}

export async function getOrCreateCustomer(
  stripe: Stripe,
  email: string,
  existingId?: string | null
): Promise<string> {
  if (existingId) return existingId;

  const customer = await stripe.customers.create({ email });
  return customer.id;
}
