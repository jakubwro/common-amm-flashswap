#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
mod flashswap {
    const TRADING_FEE_DENOM: u128 = 1000;

    use amm_helpers::{ensure, math::casted_mul};
    use ink::{
        codegen::TraitCallBuilder,
        contract_ref,
        prelude::{string::String, vec::Vec},
        storage::Mapping,
        LangError,
    };
    use psp22::PSP22;
    use scale::{Decode, Encode};
    use traits::{Factory, Pair, SwapCallee};

    #[derive(Debug, Encode, Decode)]
    pub struct SwapCallData {
        pub path: Vec<AccountId>,
        pub amounts_out: Vec<u128>,
    }

    #[ink(storage)]
    pub struct FlashSwap {
        owner: AccountId,
        factory: AccountId,
        pairs: Mapping<(AccountId, AccountId), (AccountId, u8)>,
    }

    impl FlashSwap {
        #[ink(constructor)]
        pub fn new(factory: AccountId) -> Self {
            Self {
                owner: Self::env().caller(),
                factory,
                pairs: Default::default(),
            }
        }

        #[ink(message)]
        pub fn owner(&self) -> AccountId {
            self.owner
        }

        #[ink(message)]
        pub fn set_owner(&mut self, new_owner: AccountId) -> Result<(), FlashSwapError> {
            ensure!(self.env().caller() == self.owner, FlashSwapError::CallerIsNotOwner);
            self.owner = new_owner;
            Ok(())
        }

        #[ink(message)]
        pub fn read_cache(
            &self,
            token_0: AccountId,
            token_1: AccountId,
        ) -> Option<(AccountId, u8)> {
            self.pairs.get((token_0, token_1))
        }

        #[inline]
        fn cache_pair(&mut self, pair: AccountId) {
            let pair_ref: contract_ref!(Pair) = pair.into();
            let token_0 = pair_ref.get_token_0();
            let token_1 = pair_ref.get_token_1();
            let fee = pair_ref.get_fee();
            self.pairs.insert((token_0, token_1), &(pair, fee));
            self.pairs.insert((token_1, token_0), &(pair, fee));
        }

        #[ink(message)]
        pub fn add_pair_to_cache(&mut self, pair: AccountId) -> Result<(), FlashSwapError> {
            ensure!(self.env().caller() == self.owner, FlashSwapError::CallerIsNotOwner);
            self.cache_pair(pair);
            Ok(())
        }

        #[ink(message)]
        pub fn remove_pair_from_cache(
            &mut self,
            token_0: AccountId,
            token_1: AccountId,
        ) -> Result<(), FlashSwapError> {
            ensure!(self.env().caller() == self.owner, FlashSwapError::CallerIsNotOwner);
            self.pairs.remove((token_0, token_1));
            self.pairs.remove((token_1, token_0));
            Ok(())
        }

        #[inline]
        fn factory_ref(&self) -> contract_ref!(Factory) {
            self.factory.into()
        }

        fn get_amount_out(
            &self,
            amount_in: u128,
            reserve_0: u128,
            reserve_1: u128,
            fee: u8,
        ) -> Result<u128, FlashSwapError> {
            ensure!(amount_in > 0, FlashSwapError::InsufficientAmount);
            ensure!(
                reserve_0 > 0 && reserve_1 > 0,
                FlashSwapError::InsufficientLiquidity
            );

            let amount_in_with_fee = casted_mul(amount_in, TRADING_FEE_DENOM - (fee as u128));

            let numerator = amount_in_with_fee
                .checked_mul(reserve_1.into())
                .ok_or(FlashSwapError::MulOverflow(13))?;

            let denominator = casted_mul(reserve_0, TRADING_FEE_DENOM)
                .checked_add(amount_in_with_fee)
                .ok_or(FlashSwapError::AddOverflow(2))?;

            let amount_out: u128 = numerator
                .checked_div(denominator)
                .ok_or(FlashSwapError::DivByZero(7))?
                .try_into()
                .map_err(|_| FlashSwapError::CastOverflow(4))?;

            Ok(amount_out)
        }

        fn calculate_amounts_out(
            &mut self,
            amount_in: u128,
            path: &Vec<AccountId>,
        ) -> Result<Vec<u128>, FlashSwapError> {
            ensure!(path.len() >= 2, FlashSwapError::InvalidPath);

            let mut amounts = Vec::with_capacity(path.len()+1);
            amounts.push(amount_in);
            for i in 0..path.len() - 1 {
                let (reserve_0, reserve_1, fee) = self.get_reserves(path[i], path[i + 1])?;
                amounts.push(self.get_amount_out(amounts[i], reserve_0, reserve_1, fee)?);
            }

            Ok(amounts)
        }

        fn get_pair_and_fee(
            &mut self,
            token_0: AccountId,
            token_1: AccountId,
        ) -> Result<(AccountId, u8), FlashSwapError> {
            if let Some(result) = self.pairs.get((token_0, token_1)) {
                Ok(result)
            } else {
                let pair = self
                    .factory_ref()
                    .get_pair(token_0, token_1)
                    .ok_or(FlashSwapError::PairNotFound)?;

                let pair_ref: contract_ref!(Pair) = pair.into();
                let fee = pair_ref.get_fee();

                self.pairs.insert((token_0, token_1), &(pair, fee));
                self.pairs.insert((token_1, token_0), &(pair, fee));

                Ok((pair, fee))
            }
        }

        #[inline]
        fn get_pair(
            &mut self,
            token_0: AccountId,
            token_1: AccountId,
        ) -> Result<AccountId, FlashSwapError> {
            Ok(self.get_pair_and_fee(token_0, token_1)?.0)
        }

        #[inline]
        fn get_pair_safe(
            &mut self,
            token_0: AccountId,
            token_1: AccountId,
        ) -> Result<AccountId, FlashSwapError> {
            if let Some(result) = self.pairs.get((token_0, token_1)) {
                Ok(result.0)
            } else {
                Err(FlashSwapError::PairNotFound)
            }
        }

        #[inline]
        fn get_reserves(
            &mut self,
            token_0: AccountId,
            token_1: AccountId,
        ) -> Result<(u128, u128, u8), FlashSwapError> {
            ensure!(token_0 != token_1, FlashSwapError::IdenticalAddresses);
            let (pair, fee) = self.get_pair_and_fee(token_0, token_1)?;
            let pair: contract_ref!(Pair) = pair.into();
            let (reserve_0, reserve_1, _) = pair.get_reserves();
            if token_0 < token_1 {
                Ok((reserve_0, reserve_1, fee))
            } else {
                Ok((reserve_1, reserve_0, fee))
            }
        }

        fn swap(
            &mut self,
            amounts: &[u128],
            path: &Vec<AccountId>,
            payee: AccountId,
        ) -> () {
            for i in 1..path.len() - 1 {
                let (input, output) = (path[i], path[i + 1]);
                assert!(input != output);
                let amount_out = amounts[i + 1];
                let (amount_0_out, amount_1_out) = if input < output {
                    (0, amount_out)
                } else {
                    (amount_out, 0)
                };
                let to = if i < path.len() - 2 {
                    self.get_pair_safe(output, path[i + 2]).unwrap()
                } else {
                    payee
                };
                let mut pair: contract_ref!(Pair) = self.get_pair_safe(input, output).unwrap().into();
                pair.swap(amount_0_out, amount_1_out, to, None).unwrap();
            }
            ()
        }

        #[ink(message)]
        pub fn draw(
            &mut self,
            token: AccountId,
        )-> Result<(), FlashSwapError> {
            ensure!(self.env().caller() == self.owner, FlashSwapError::CallerIsNotOwner);

            let mut token_ref: contract_ref!(PSP22) = token.into();
            let value = token_ref.balance_of(self.env().account_id());
            token_ref.transfer(self.owner, value, Vec::new()).unwrap();

            Ok(())
        }

        #[ink(message)]
        pub fn flashswap(
            &mut self,
            amounts: Vec<u128>,
            path: Vec<AccountId>,
        ) -> Result<Vec<u128>, FlashSwapError> {
            ensure!(self.env().caller() == self.owner, FlashSwapError::CallerIsNotOwner);
            let amount = amounts[0];
            ensure!(amount > 0, FlashSwapError::AmountIsZero);
            ensure!(path.len() > 2, FlashSwapError::InvalidPath);
            ensure!(path[0] == path[path.len() - 1], FlashSwapError::PathAcyclic);

            let amounts_out = if amounts.len() == 1 {
                self.calculate_amounts_out(amount, &path)?
            } else {
                amounts
            };

            let received = amounts_out[amounts_out.len() - 1];
            ensure!(received > amount, FlashSwapError::NoProfit);

            let borrow_token_id = path[0];
            let paired_token_id = path[1];

            let pair_id = self.get_pair(borrow_token_id, paired_token_id)?;

            let mut pair: contract_ref!(Pair) = pair_id.into();

            let amount_out = amounts_out[1];
            let (amount_0_out, amount_1_out) = if borrow_token_id < paired_token_id {
                (0, amount_out)
            } else {
                (amount_out, 0)
            };

            let data = SwapCallData {
                path,
                amounts_out: amounts_out.clone(),
            };

            pair.call_mut()
                .swap(
                    amount_0_out,
                    amount_1_out,
                    self.env().account_id(),
                    Some(data.encode()),
                )
                .call_flags(ink_env::CallFlags::default().set_allow_reentry(true))
                .try_invoke()
                .map_err(|_| FlashSwapError::SwapCallFailed)??
                .unwrap();

            Ok(amounts_out)
        }
    }

    impl SwapCallee for FlashSwap {
        #[ink(message)]
        fn swap_call(&mut self, _sender: AccountId, amount0: u128, amount1: u128, data: Vec<u8>) {
            let SwapCallData { path, amounts_out } =
            SwapCallData::decode(&mut &data[..]).ok().unwrap();
            
            assert!(path[0] == path[path.len() - 1]);
            assert!(amounts_out[0] < amounts_out[amounts_out.len() - 1]);
            let borrow_token_id = path[0];
            let paired_token_id = path[1];

            let pair_id: ink::primitives::AccountId = self.get_pair_safe(borrow_token_id, paired_token_id).unwrap();
            assert!(self.env().caller() == pair_id);
            
            let mut borrow_token: contract_ref!(PSP22) = borrow_token_id.into();
            let mut paired_token: contract_ref!(PSP22) = paired_token_id.into();

            assert!(amount0 == amounts_out[1] || amount1 == amounts_out[1]);
            let next_pair = self.get_pair_safe(path[1], path[2]).unwrap();
            paired_token
                .transfer(next_pair, amounts_out[1], Vec::new())
                .unwrap();

            self.swap(&amounts_out, &path, self.env().account_id());

            borrow_token
            .transfer(pair_id, amounts_out[0], Vec::new())
            .unwrap();
        }
    }

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum FlashSwapError {
        LangError(LangError),
        SomeError,
        AmountIsZero,
        PathAcyclic,
        InvalidPath,
        IdenticalAddresses,
        InsufficientAmount,
        InsufficientLiquidity,
        PairNotFound,
        NoProfit,
        SwapCallFailed,
        CallerIsNotOwner,

        AddOverflow(u8),
        CastOverflow(u8),
        DivByZero(u8),
        MulOverflow(u8),
        SubUnderflow(u8),

        CrossContractCallFailed(String),
    }

    macro_rules! impl_froms {
        ( $( $error:ident ),* ) => {
            $(
                impl From<$error> for FlashSwapError {
                    fn from(error: $error) -> Self {
                        FlashSwapError::$error(error)
                    }
                }
            )*
        };
    }

    impl_froms!(LangError);
}
