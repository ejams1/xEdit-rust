{ parity probe: x-raise-27 }
unit P_x_raise_27;

function Initialize: integer;
begin
  raise EDivByZero.Create('dz');
end;
end.
