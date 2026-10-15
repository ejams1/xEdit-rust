{ parity probe: x-raise-30 }
unit P_x_raise_30;

function Initialize: integer;
begin
  raise EVariantError.Create('ve');
end;
end.
