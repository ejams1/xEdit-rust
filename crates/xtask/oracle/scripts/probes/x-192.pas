{ parity probe: x-192 }
unit P_x_192;

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
  v := -'x';
  AddMessage('after');
end;
end.
