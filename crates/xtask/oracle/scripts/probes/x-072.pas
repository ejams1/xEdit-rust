{ parity probe: x-072 }
unit P_x_072;

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
  v := StrToInt64('x');
  AddMessage('after');
end;
end.
