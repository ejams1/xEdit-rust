{ parity probe: x-raise-07 }
unit P_x_raise_07;

function Initialize: integer;
var
  a: array[0..1] of Integer;
begin
  try
    a[5] := 1;
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
