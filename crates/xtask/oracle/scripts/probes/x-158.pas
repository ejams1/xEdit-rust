{ parity probe: x-158 }
unit P_x_158;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := 'abc'; v := v > 4;
  AddMessage('after');
end;
end.
