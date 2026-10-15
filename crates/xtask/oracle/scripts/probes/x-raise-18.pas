{ parity probe: x-raise-18 }
unit P_x_raise_18;

function Initialize: integer;
var
  s: string;
begin
  try
    s := Format('%d', [1.5]);
  except
    on E: EConvertError do AddMessage('conv: ' + E.Message);
  end;
  AddMessage('after');
end;
end.
