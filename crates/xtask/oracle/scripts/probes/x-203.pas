{ parity probe: x-203 }
unit P_x_203;

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
  v := nil + 'x';
  AddMessage('after');
end;
end.
