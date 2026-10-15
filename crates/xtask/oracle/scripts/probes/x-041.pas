{ parity probe: x-041 }
unit P_x_041;

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
  s := 'abc'; AddMessage(s[2]);
  AddMessage('after');
end;
end.
