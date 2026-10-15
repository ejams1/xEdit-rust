{ parity probe: x-378 }
unit P_x_378;

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
  v := TList.Create; v.Add(nil); AddMessage(IntToStr(v.IndexOf(0)));
  AddMessage('after');
end;
end.
