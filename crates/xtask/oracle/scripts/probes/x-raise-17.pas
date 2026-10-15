{ parity probe: x-raise-17 }
unit P_x_raise_17;

function Initialize: integer;
var
  a: array[0..1] of Integer;
begin
  try
    a[5] := 1;
  except
    on E: EJvInterpreterError do AddMessage('jvi ' + IntToStr(E.ErrCode) + ' ' + E.Message);
  end;
  AddMessage('after');
end;
end.
