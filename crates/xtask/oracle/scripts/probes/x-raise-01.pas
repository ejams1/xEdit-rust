{ parity probe: x-raise-01 }
unit P_x_raise_01;

function Initialize: integer;
begin
  raise EConvertError.Create('conv');
end;
end.
