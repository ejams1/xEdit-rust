{ parity probe: x-raise-28 }
unit P_x_raise_28;

function Initialize: integer;
begin
  raise EInvalidOp.Create('io');
end;
end.
