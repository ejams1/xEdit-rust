{ parity probe: x-raise-15 }
unit P_x_raise_15;

function Initialize: integer;
begin
  try
    AddMessage('body');
  finally
    AddMessage('finally');
  end;
  AddMessage('after');
end;
end.
